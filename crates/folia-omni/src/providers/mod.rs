// Registry of the platform providers compiled in; each platform is a cargo feature named after its id.

#[cfg(feature = "kugou")]
mod kugou;
#[cfg(feature = "netease")]
mod netease;

use crate::Provider;

pub fn all(http: &reqwest::Client) -> Vec<Box<dyn Provider>> {
    vec![
        #[cfg(feature = "netease")]
        Box::new(netease::Netease::new(http.clone())),
        #[cfg(feature = "kugou")]
        Box::new(kugou::Kugou::new(http.clone())),
    ]
}
