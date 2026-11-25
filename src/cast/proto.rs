//! The one message of the Cast protocol (`cast_channel.proto`), which wraps every payload.
//! The fields are optional in this definition so they are always written: the device wants the
//! ones that are zero too.

#[derive(Clone, PartialEq, prost::Message)]
pub struct CastMessage {
    #[prost(int32, optional, tag = "1")]
    pub protocol_version: Option<i32>,
    #[prost(string, optional, tag = "2")]
    pub source_id: Option<String>,
    #[prost(string, optional, tag = "3")]
    pub destination_id: Option<String>,
    #[prost(string, optional, tag = "4")]
    pub namespace: Option<String>,
    /// 0 is a string payload.
    #[prost(int32, optional, tag = "5")]
    pub payload_type: Option<i32>,
    #[prost(string, optional, tag = "6")]
    pub payload_utf8: Option<String>,
}
