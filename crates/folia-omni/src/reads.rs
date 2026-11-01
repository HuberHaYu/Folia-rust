// What explore adapters share for reading their page anew: what the reads before handed out (`exploreReads`).

use std::collections::HashMap;

/// A scope remembers this many keys; past that the one used longest ago is forgotten (`MAX_KEPT`).
const MAX_KEPT: usize = 5000;

/// `createHandedOut`: by scope, each key with its place in the order of use.
#[derive(Default)]
pub(crate) struct HandedOut {
    scopes: HashMap<String, HashMap<String, u64>>,
    uses: u64,
}

impl HandedOut {
    /// `inTurn`: the candidates never used in `scope` first, in their order, then the others, oldest use first.
    pub(crate) fn in_turn<T>(&self, scope: &str, mut candidates: Vec<T>, key_of: impl Fn(&T) -> String) -> Vec<T> {
        if let Some(used_at) = self.scopes.get(scope) {
            candidates.sort_by_cached_key(|candidate| used_at.get(&key_of(candidate)).copied());
        }
        candidates
    }

    /// `note`: `keys` were used in `scope`, now and in this order.
    pub(crate) fn note(&mut self, scope: &str, keys: impl IntoIterator<Item = String>) {
        let used_at = self.scopes.entry(scope.to_owned()).or_default();
        for key in keys {
            self.uses += 1;
            used_at.insert(key, self.uses);
        }
        while used_at.len() > MAX_KEPT {
            let Some(oldest) = used_at.iter().min_by_key(|(_, place)| **place).map(|(key, _)| key.clone()) else {
                break;
            };
            used_at.remove(&oldest);
        }
    }
}
