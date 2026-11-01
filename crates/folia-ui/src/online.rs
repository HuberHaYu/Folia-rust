// The online account and the home tabs' online data, all through the `Omni` facade and off the UI thread.

use std::collections::HashSet;

use folia_omni::{Account, Collection, ExploreSection, Omni, RadioFeed};
use iced::Task;

use crate::background::{blocking, error_chain};

/// The platform used when none was chosen yet or the one kept is not compiled in.
const FIRST_PROVIDER: &str = "netease";
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
    /// Each available platform, and whether a kept session was read into it.
    Restored(Vec<(&'static str, Result<bool, String>)>),
    Account(&'static str, Result<Option<Account>, String>),
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
    /// The platform the online tabs show (`activeProviderId`).
    provider: &'static str,
    /// Platforms holding a session read from the keyring or signed in during this run.
    kept: HashSet<&'static str>,
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
    /// Reads every platform's kept session from the system keyring, then asks who the chosen one belongs to.
    pub fn new(omni: Option<Omni>, preferred: Option<&str>) -> (Self, Task<Message>) {
        let available: Vec<&'static str> = omni
            .as_ref()
            .map(|omni| omni.platforms().into_iter().filter(|p| p.available).map(|p| p.id).collect())
            .unwrap_or_default();
        let provider = preferred
            .and_then(|wanted| available.iter().copied().find(|id| *id == wanted))
            .or_else(|| available.first().copied())
            .unwrap_or(FIRST_PROVIDER);
        let task = match omni.clone() {
            Some(omni) => {
                let ids = available.clone();
                let work = blocking(move || {
                    ids.into_iter()
                        .map(|id| (id, omni.restore_session(id).map_err(|err| error_chain(&err))))
                        .collect::<Vec<_>>()
                });
                Task::perform(work, |result| Message::Restored(result.unwrap_or_default()))
            }
            None => Task::none(),
        };
        let account = if omni.is_some() { AccountState::Resolving } else { AccountState::Guest { ran_out: false } };
        let online = Self {
            omni,
            provider,
            kept: HashSet::new(),
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

    /// The platform the online tabs show.
    pub fn provider(&self) -> &'static str {
        self.provider
    }

    /// Platforms with a session in memory: they can be switched to without a new sign-in.
    pub fn kept(&self) -> &HashSet<&'static str> {
        &self.kept
    }

    /// `requestSwitch`: shows another platform whose session is kept, reading its account anew.
    pub fn switch_to(&mut self, provider: &'static str) -> Task<Message> {
        if provider == self.provider || !self.kept.contains(provider) {
            return Task::none();
        }
        tracing::info!(provider, "online platform switched");
        self.provider = provider;
        self.restored = true;
        self.account = AccountState::Resolving;
        self.avatar = None;
        self.reset_feeds();
        self.read_account()
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
            Message::Restored(results) => {
                for (provider, result) in results {
                    match result {
                        Ok(true) => {
                            self.kept.insert(provider);
                        }
                        Ok(false) => {}
                        Err(err) => tracing::warn!(provider, error = %err, "no kept online session"),
                    }
                }
                self.restored = self.kept.contains(self.provider);
                if self.restored {
                    return (self.read_account(), false);
                }
                self.account = AccountState::Guest { ran_out: false };
                (Task::none(), true)
            }
            Message::Account(provider, _) if provider != self.provider => (Task::none(), false),
            Message::Account(provider, Ok(Some(account))) => {
                tracing::info!(provider, "online account read");
                self.account = AccountState::SignedIn(account.clone());
                self.reset_feeds();
                (Task::batch([self.load_playlists(&account), self.load_avatar(&account)]), true)
            }
            Message::Account(provider, Ok(None)) => {
                // `signInRanOut`: a kept session that reads as nobody is dropped.
                let ran_out = self.restored;
                self.restored = false;
                self.kept.remove(provider);
                self.account = AccountState::Guest { ran_out };
                self.reset_feeds();
                (self.forget(), true)
            }
            Message::Account(_, Err(err)) => {
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
                if !self.explore_refreshing || self.owner().is_none_or(|current| current != owner) {
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
                // `exchanged`: a section that stands takes what it has now in its place, one the platform no longer
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
                if self.owner().is_some_and(|current| current == owner) {
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
        let owner = self.owner_of(account);
        Task::perform(crate::background::online_cover(omni, url, AVATAR_PX), move |cover| {
            Message::Avatar(owner.clone(), cover)
        })
    }

    /// After a confirmed sign-in on `provider`: it becomes the platform in use, its session is kept, its account read.
    pub fn signed_in_now(&mut self, provider: &'static str) -> Task<Message> {
        let Some(omni) = self.omni.clone() else { return Task::none() };
        if provider != self.provider {
            self.provider = provider;
            self.avatar = None;
            self.reset_feeds();
        }
        self.kept.insert(provider);
        self.restored = false;
        let keep = blocking(move || omni.save_session(provider).map_err(|err| error_chain(&err)));
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
        let provider = self.provider;
        self.account = AccountState::Guest { ran_out: false };
        self.restored = false;
        self.kept.remove(provider);
        self.reset_feeds();
        let work = async move {
            if let Err(err) = omni.logout(provider).await {
                tracing::warn!(error = &err as &dyn std::error::Error, "platform logout failed");
            }
            let forget = omni.clone();
            if let Some(Err(err)) = blocking(move || forget.forget_session(provider)).await {
                tracing::error!(error = &err as &dyn std::error::Error, "cannot remove the kept session");
            }
        };
        Task::perform(work, |()| Message::Forgotten)
    }

    /// Reads what an online tab shows the first time it is opened for this account (`attachOnlineHomeFeeds`).
    pub fn ensure(&mut self, tab: folia_store::HomeTab) -> Task<Message> {
        let (Some(omni), Some(account)) = (self.omni.clone(), self.signed_in().cloned()) else { return Task::none() };
        let (owner, provider) = (self.owner_of(&account), self.provider);
        match tab {
            folia_store::HomeTab::Albums if self.albums == Feed::Idle => {
                self.albums = Feed::Loading;
                Task::perform(
                    async move { omni.favorite_albums(provider).await.map_err(|err| error_chain(&err)) },
                    move |r| Message::Albums(owner.clone(), r),
                )
            }
            folia_store::HomeTab::Radio if self.radio == Feed::Idle => {
                self.radio = Feed::Loading;
                Task::perform(
                    async move { omni.radio_feed(provider, RADIO_LIMIT).await.map_err(|err| error_chain(&err)) },
                    move |r| Message::Radio(owner.clone(), r),
                )
            }
            folia_store::HomeTab::Explore if self.explore == Feed::Idle => {
                self.explore = Feed::Loading;
                Task::perform(async move { omni.explore(provider).await.map_err(|err| error_chain(&err)) }, move |r| {
                    Message::Explore(owner.clone(), r)
                })
            }
            folia_store::HomeTab::Playlist if self.playlists == Feed::Idle => self.load_playlists(&account),
            _ => Task::none(),
        }
    }

    /// 「换一批」 (`refresh`): the platform's page is read anew, every shelf handing out what it has not shown yet first.
    pub fn refresh_explore(&mut self) -> Task<Message> {
        let (Some(omni), Some(account)) = (self.omni.clone(), self.signed_in().cloned()) else { return Task::none() };
        if self.explore_refreshing || self.explore == Feed::Loading || self.explore == Feed::Idle {
            return Task::none();
        }
        self.explore_refreshing = true;
        let (owner, provider) = (self.owner_of(&account), self.provider);
        Task::perform(async move { omni.explore(provider).await.map_err(|err| error_chain(&err)) }, move |r| {
            Message::ExploreRefreshed(owner.clone(), r)
        })
    }

    pub fn can_refresh_explore(&self) -> bool {
        !self.explore_refreshing && matches!(self.explore, Feed::Ready(_) | Feed::Failed(_))
    }

    fn load_playlists(&mut self, account: &Account) -> Task<Message> {
        let Some(omni) = self.omni.clone() else { return Task::none() };
        self.playlists = Feed::Loading;
        let (owner, user, provider) = (self.owner_of(account), account.id.clone(), self.provider);
        Task::perform(
            async move { omni.user_playlists(provider, &user).await.map_err(|err| error_chain(&err)) },
            move |result| Message::Playlists(owner.clone(), result),
        )
    }

    fn read_account(&mut self) -> Task<Message> {
        let Some(omni) = self.omni.clone() else { return Task::none() };
        self.account = AccountState::Resolving;
        let provider = self.provider;
        Task::perform(async move { omni.account(provider).await.map_err(|err| error_chain(&err)) }, move |result| {
            Message::Account(provider, result)
        })
    }

    fn forget(&self) -> Task<Message> {
        let Some(omni) = self.omni.clone() else { return Task::none() };
        let provider = self.provider;
        Task::perform(blocking(move || omni.forget_session(provider)), |_| Message::Forgotten)
    }

    /// Whose answers are kept: the platform and the account signed in on it.
    fn owner(&self) -> Option<String> {
        self.signed_in().map(|account| self.owner_of(account))
    }

    fn owner_of(&self, account: &Account) -> String {
        format!("{}:{}", self.provider, account.id)
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
        if self.owner().is_none_or(|current| current != owner) {
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
