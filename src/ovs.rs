use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

#[skip_serializing_none()]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct OvsBridge {
    pub vlan: Option<OvsBridgeVlan>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct OvsBridgeVlan {
    pub parent: String,
    pub tag: u16,
}
