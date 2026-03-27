use crate::{DummyRequest, DummyResponse};

#[test]
fn should_deser() {
    let request = DummyRequest {
        input: "Hello".to_string(),
    };
    let encoded = candid::encode_one(&request).unwrap();
    let decoded: DummyRequest = candid::decode_one(&encoded).unwrap();
    assert_eq!(request, decoded);

    let response = DummyResponse {
        output: "Hello world!".to_string(),
    };
    let encoded = candid::encode_one(&response).unwrap();
    let decoded: DummyResponse = candid::decode_one(&encoded).unwrap();
    assert_eq!(response, decoded);
}
