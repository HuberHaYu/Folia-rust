// The online account and the home tabs' online data, all through the `Omni` facade and off the UI thread.

use folia_omni::{Account, Collection, ExploreSection, Omni, RadioFeed};
use iced::Task;

use crate::background::{blocking, error_chain};

/// The only platform with a provider so far.
pub const PROVIDER: &str = "netease";
/// `HOME_FEED_RECOMMENDATION_LIMIT`.
const RADIO_LIMIT: u32 = 35;

/// Whose account the online tabs show (`accountView`).
#[derive(Debug, Clone, PartialEq)]
pub enum AccountState {
    /// The kept session is being read and checked.
    Resolving,
    /// Nobody is signed in; `ran_out` when a kept session no longer works (`needsRelogin`).
    Guest {
        ran_out: bool,
    },
    SignedIn(Account),
}

/// One piece of online data: not asked for, on its way, here, or failed.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Feed<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> Feed<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Feed::Ready(value) => Some(value),
            _ => None,
        }
    }

    pub fn loading(&self) -> bool {
        matches!(self, Feed::Loading)
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Restored(Result<bool, String>),
    Account(Result<Option<Account>, String>),
    Playlists(String, Result<Vec<Collection>, String>),
    Albums(String, Result<Vec<Collection>, String>),
    Radio(String, Result<RadioFeed, String>),
    Explore(String, Result<Vec<ExploreSection>, String>),
    /// The answer to 「换一批」.
    ExploreRefreshed(String, Result<Vec<ExploreSection>, String>),
    Avatar(String, Option<folia_library::Cover>),
    Forgotten,
}

/// The account picture is decoded at twice the 32px it is drawn at.
const AVATAR_PX: u32 = 64;

pub struct Online {
    omni: Option<Omni>,
    pub account: AccountState,
    restored: bool,
    pub playlists: Feed<Vec<Collection>>,
    pub albums: Feed<Vec<Collection>>,
    pub radio: Feed<RadioFeed>,
    pub explore: Feed<Vec<ExploreSection>>,
    /// 「换一批」 is on its way; the page on screen stays until it arrives.
    pub explore_refreshing: bool,
    pub avatar: Option<iced::widget::image::Handle>,
}

impl Online {
    /// Reads the kept session from the system keyring, then asks who it belongs to.
    pub fn new(omni: Option<Omni>) -> (Self, Task<Message>) {
        let task = match omni.clone() {
            Some(omni) => {
                let work = blocking(move || omni.restore_session(PROVIDER).map_err(|err| error_chain(&err)));
                Task::perform(work, |result| {
                    Message::Restored(result.unwrap_or_else(|| Err("keyring worker stopped".to_owned())))
                })
            }
            None => Task::none(),
        };
        let account = if omni.is_some() { AccountState::Resolving } else { AccountState::Guest { ran_out: false } };
        let online = Self {
            omni,
            account,
            restored: false,
            playlists: Feed::Idle,
            albums: Feed::Idle,
            radio: Feed::Idle,
            explore: Feed::Idle,
            explore_refreshing: false,
            avatar: None,
        };
        (online, task)
    }

    pub fn omni(&self) -> Option<&Omni> {
        self.omni.as_ref()
    }

    pub fn signed_in(&self) -> Option<&Account> {
        match &self.account {
            AccountState::SignedIn(account) => Some(account),
            _ => None,
        }
    }

    /// Returns whether the account changed, so the wall can be rebuilt.
    pub fn update(&mut self, message: Message) -> (Task<Message>, bool) {
        match message {
            Message::Restored(result) => {
                self.restored = matches!(result, Ok(true));
                match result {
                    Ok(true) => (self.read_account(), false),
                    Ok(false) => {
                        self.account = AccountState::Guest { ran_out: false };
                        (Task::none(), true)
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "no kept online session");
                        self.account = AccountState::Guest { ran_out: false };
                        (Task::none(), true)
                    }
                }
            }
            Message::Account(Ok(Some(account))) => {
                tracing::info!(provider = PROVIDER, "online account read");
                self.account = AccountState::SignedIn(account.clone());
                self.reset_feeds();
                (Task::batch([self.load_playlists(&account), self.load_avatar(&account)]), true)
            }
            Message::Account(Ok(None)) => {
                // `signInRanOut`: a kept session that reads as nobody is dropped.
                let ran_out = self.restored;
                self.restored = false;
                self.account = AccountState::Guest { ran_out };
                self.reset_feeds();
                (self.forget(), true)
            }
            Message::Account(Err(err)) => {
                tracing::warn!(error = %err, "online account unreadable");
                self.account = AccountState::Guest { ran_out: false };
                (Task::none(), true)
            }
            Message::Playlists(owner, result) => {
                (Task::none(), self.settle(&owner, result, |online| &mut online.playlists))
            }
            Message::Albums(owner, result) => (Task::none(), self.settle(&owner, result, |online| &mut online.albums)),
            Message::Radio(owner, result) => (Task::none(), self.settle(&owner, result, |online| &mut online.radio)),
            Message::Explore(owner, result) => {
                (Task::none(), self.settle(&owner, result, |online| &mut online.explore))
            }
            Message::ExploreRefreshed(owner, result) => {
                if !self.explore_refreshing || self.signed_in().is_none_or(|account| account.id != owner) {
                    return (Task::none(), false);
                }
                self.explore_refreshing = false;
                let fresh = match result {
                    Ok(fresh) => fresh,
                    Err(err) => {
                        tracing::warn!(error = %err, "explore refresh unavailable");
                        return (Task::none(), false);
                    }
                };
                let Feed::Ready(shown) = &mut self.explore else {
                    self.explore = Feed::Ready(fresh);
                    return (Task::none(), true);
                };
                // `exchanged`: a section that stands takes what it has now in its place, one NetEase no longer
                // names keeps what it showed, and the new ones come after.
                let mut joining = Vec::new();
                for section in fresh {
                    match shown.iter_mut().find(|standing| standing.key == section.key) {
                        Some(standing) => *standing = section,
                        None => joining.push(section),
                    }
                }
                shown.extend(joining);
                (Task::none(), true)
            }
            Message::Avatar(owner, cover) => {
                if self.signed_in().is_some_and(|account| account.id == owner) {
                    self.avatar = cover
                        .map(|cover| iced::widget::image::Handle::from_rgba(cover.width, cover.height, cover.rgba));
                }
                (Task::none(), false)
            }
            Message::Forgotten => (Task::none(), false),
        }
    }

    fn load_avatar(&mut self, account: &Account) -> Task<Message> {
        self.avatar = None;
        let (Some(omni), Some(url)) = (self.omni.clone(), account.avatar_url.clone()) else { return Task::none() };
        let owner = account.id.clone();
        Task::perform(crate::background::online_cover(omni, url, AVATAR_PX), move |cover| {
            Message::Avatar(owner.clone(), cover)
        })
    }

    /// After a confirmed sign-in: keep the session, then read the account.
    pub fn signed_in_now(&mut self) -> Task<Message> {
        let Some(omni) = self.omni.clone() else { return Task::none() };
        self.restored = false;
        let keep = blocking(move || omni.save_session(PROVIDER).map_err(|err| error_chain(&err)));
        let keep = Task::perform(keep, |result| {
            if let Some(Err(err)) = result {
                tracing::error!(error = %err, "cannot keep the online session");
            }
            Message::Forgotten
        });
        Task::batch([keep, self.read_account()])
    }

    /// `logout`: tells the platform, drops the kept session, and shows the guest wall.
    pub fn sign_out(&mut self) -> Task<Message> {
        let Some(omni) = self.omni.clone() else { return Task::none() };
        self.account = AccountState::Guest { ran_out: false };
        self.restored = false;
        self.reset_feeds();
        let work = async move {
            if let Err(err) = omni.logout(PROVIDER).await {
                tracing::warn!(error = &err as &dyn std::error::Error, "platform logout failed");
            }
            let forget = omni.clone();
            if let Some(Err(err)) = blocking(move || forget.forget_session(PROVIDER)).await {
                tracing::error!(error = &err as &dyn std::error::Error, "cannot remove the kept session");
            }
        };
        Task::perform(work, |()| Message::Forgotten)
    }

    /// Reads what an online tab shows the first time it is opened for this account (`attachOnlineHomeFeeds`).
    pub fn ensure(&mut self, tab: folia_store::HomeTab) -> Task<Message> {
        let (Some(omni), Some(account)) = (self.omni.clone(), self.signed_in().cloned()) else { return Task::none() };
        let owner = account.id.clone();
        match tab {
            folia_store::HomeTab::Albums if self.albums == Feed::Idle => {
                self.albums = Feed::Loading;
                Task::perform(
                    async move { omni.favorite_albums(PROVIDER).await.map_err(|err| error_chain(&err)) },
                    move |r| Message::Albums(owner.clone(), r),
                )
            }
            folia_store::HomeTab::Radio if self.radio == Feed::Idle => {
                self.radio = Feed::Loading;
                Task::perform(
                    async move { omni.radio_feed(PROVIDER, RADIO_LIMIT).await.map_err(|err| error_chain(&err)) },
                    move |r| Message::Radio(owner.clone(), r),
                )
            }
            folia_store::HomeTab::Explore if self.explore == Feed::Idle => {
                self.explore = Feed::Loading;
                Task::perform(async move { omni.explore(PROVIDER).await.map_err(|err| error_chain(&err)) }, move |r| {
                    Message::Explore(owner.clone(), r)
                })
            }
            folia_store::HomeTab::Playlist if self.playlists == Feed::Idle => self.load_playlists(&account),
            _ => Task::none(),
        }
    }

    /// 「换一批」 (`refresh`): NetEase's page is read anew, every shelf handing out what it has not shown yet first.
    pub fn refresh_explore(&mut self) -> Task<Message> {
        let (Some(omni), Some(account)) = (self.omni.clone(), self.signed_in().cloned()) else { return Task::none() };
        if self.explore_refreshing || self.explore == Feed::Loading || self.explore == Feed::Idle {
            return Task::none();
        }
        self.explore_refreshing = true;
        let owner = account.id;
        Task::perform(async move { omni.explore(PROVIDER).await.map_err(|err| error_chain(&err)) }, move |r| {
            Message::ExploreRefreshed(owner.clone(), r)
        })
    }

    pub fn can_refresh_explore(&self) -> bool {
        !self.explore_refreshing && matches!(self.explore, Feed::Ready(_) | Feed::Failed(_))
    }

    fn load_playlists(&mut self, account: &Account) -> Task<Message> {
        let Some(omni) = self.omni.clone() else { return Task::none() };
        self.playlists = Feed::Loading;
        let (owner, user) = (account.id.clone(), account.id.clone());
        Task::perform(
            async move { omni.user_playlists(PROVIDER, &user).await.map_err(|err| error_chain(&err)) },
            move |result| Message::Playlists(owner.clone(), result),
        )
    }

    fn read_account(&mut self) -> Task<Message> {
        let Some(omni) = self.omni.clone() else { return Task::none() };
        self.account = AccountState::Resolving;
        Task::perform(async move { omni.account(PROVIDER).await.map_err(|err| error_chain(&err)) }, Message::Account)
    }

    fn forget(&self) -> Task<Message> {
        let Some(omni) = self.omni.clone() else { return Task::none() };
        Task::perform(blocking(move || omni.forget_session(PROVIDER)), |_| Message::Forgotten)
    }

    fn reset_feeds(&mut self) {
        if self.signed_in().is_none() {
            self.avatar = None;
        }
        self.playlists = Feed::Idle;
        self.albums = Feed::Idle;
        self.radio = Feed::Idle;
        self.explore = Feed::Idle;
        self.explore_refreshing = false;
    }

    /// Stores an answer unless it belongs to an account that is no longer signed in; returns whether it was kept.
    fn settle<T>(
        &mut self,
        owner: &str,
        result: Result<T, String>,
        feed: impl FnOnce(&mut Self) -> &mut Feed<T>,
    ) -> bool {
        if self.signed_in().is_none_or(|account| account.id != owner) {
            return false;
        }
        *feed(self) = match result {
            Ok(value) => Feed::Ready(value),
            Err(err) => {
                tracing::warn!(error = %err, "online home data unavailable");
                Feed::Failed(err)
            }
        };
        true
    }
}
