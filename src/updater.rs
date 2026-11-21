use crate::net::Net;
use anyhow::Result;
use serde::Deserialize;
#[derive(Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub html_url: String,
}
pub fn latest(net: &Net) -> Result<Release> {
    net.get_json(&[
        "https://api.github.com/repos/dogoran-el/arizona-launcher/releases/latest".into(),
    ])
}
