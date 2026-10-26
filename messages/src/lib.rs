use quick_protobuf::serialize_into_vec;
use std::convert::TryInto;

pub mod packets;
pub mod proto;

#[macro_export]
macro_rules! impl_into_packet {
    ($type:ty, $id:path) => {
        impl<'a> std::convert::TryFrom<$type> for Packet<'a> {
            type Error = quick_protobuf::Error;

            fn try_from(packet: $type) -> Result<Self, Self::Error> {
                Ok($crate::packets::Packet {
                    packet_id: $id,
                    bytes: std::borrow::Cow::Owned(quick_protobuf::serialize_into_vec(&packet)?),
                })
            }
        }
    };
}

pub fn try_into_packet<'a, T>(value: T) -> Result<Vec<u8>, quick_protobuf::Error>
where
    T: TryInto<packets::Packet<'a>, Error = quick_protobuf::Error>,
{
    T::try_into(value).and_then(|packet| serialize_into_vec(&packet))
}

#[cfg(test)]
mod lr_tests {
    use super::*;
    use quick_protobuf::{serialize_into_vec, deserialize_from_slice};
    #[test] fn scoped_wire_round_trip_preserves_generation_and_exact_json() {
        let event = packets::EmitEvent { event_name: "lr:house:ready".into(), args: Some(r#"{"text":"a b\\c\\u00e4"}"#.into()), arguments: Vec::new(), browser_id: Some(54), browser_generation: Some(23) };
        let bytes = serialize_into_vec(&event).unwrap();
        let decoded: packets::EmitEvent = deserialize_from_slice(&bytes).unwrap();
        assert_eq!(decoded, event);
        let open = packets::OpenConnection { bind_token: "abcdef0123456789".repeat(4).into() };
        let bytes = serialize_into_vec(&open).unwrap();
        let decoded: packets::OpenConnection = deserialize_from_slice(&bytes).unwrap(); assert_eq!(decoded, open);
    }
}
