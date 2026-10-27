use base64::{prelude::BASE64_STANDARD, Engine};
use clipsync_core::envelope::{
    ClipEnvelope, ClipPayload, FilePayload, PresenceEnvelope, PresencePayload,
};
use clipsync_core::protocol::{
    client_id, clip_topic, parse_topic, presence_topic, random_mid, room_filter, room_hash,
    safety_number, sync_topic, Channel,
};
use clipsync_core::room::RoomCrypto;

const ROOM: &str = "TESTROOM01";
const AAD: &[u8] = b"cs/v1/prefixclip";
const BROWSER_WIRE: &str =
    "AAECAwQFBgcICQoL+gx2TX+MMFr0F0nlliSlOqTsWYyEZTm3yisCAhw8KjnBqt5N2xJ/xzCxxu9VZLqQH7g=";
const BROWSER_TEXT: &str = "{\"text\":\"hello clipsync\",\"len\":15}";

fn key_hex(room: &str, password: &str) -> String {
    hex::encode(RoomCrypto::open(room, password).unwrap().key_material())
}

#[test]
fn room_key_matches_the_openssl_pinned_vectors() {
    assert_eq!(
        key_hex(ROOM, ""),
        "b1b738d23632bb6c4dc8bf9e235d20261e504d80030fc6f998a616ea8c790ee9"
    );
    assert_eq!(
        key_hex(ROOM, "hunter2"),
        "0c507ff2ecd664e9c25d92bc01ab781e257e2a85c7f72b9b911ccdbd1513e16e"
    );
    assert_eq!(
        key_hex("  TESTROOM01  ", ""),
        key_hex(ROOM, ""),
        "room code is trimmed before derivation"
    );
}

#[test]
fn room_hash_matches_webcrypto() {
    assert_eq!(room_hash(ROOM), "61232506");
}

#[test]
fn decrypts_the_webcrypto_generated_wire() {
    let crypto = RoomCrypto::open(ROOM, "").unwrap();
    let plain = crypto.decrypt(BROWSER_WIRE, AAD).unwrap();
    assert_eq!(String::from_utf8(plain).unwrap(), BROWSER_TEXT);
}

#[test]
fn encrypts_the_webcrypto_wire_byte_for_byte() {
    let crypto = RoomCrypto::open(ROOM, "").unwrap();
    let iv: Vec<u8> = (0u8..12).collect();
    let wire = crypto
        .encrypt_bytes_with_iv(&iv, BROWSER_TEXT.as_bytes(), AAD)
        .unwrap();
    assert_eq!(BASE64_STANDARD.encode(wire), BROWSER_WIRE);
}

#[test]
fn wrong_password_cannot_decrypt() {
    let alice = RoomCrypto::open(ROOM, "hunter2").unwrap();
    let eve = RoomCrypto::open(ROOM, "wrong").unwrap();
    let wire = alice.encrypt(b"secret", AAD).unwrap();
    assert!(eve.decrypt(&wire, AAD).is_err());
}

#[test]
fn roundtrip_is_utf8_safe() {
    let alice = RoomCrypto::open(ROOM, "hunter2").unwrap();
    let bob = RoomCrypto::open(ROOM, "hunter2").unwrap();
    let wire = alice.encrypt("跨端同步".as_bytes(), AAD).unwrap();
    assert_eq!(
        String::from_utf8(bob.decrypt(&wire, AAD).unwrap()).unwrap(),
        "跨端同步"
    );
}

#[test]
fn restored_key_interoperates_with_derived_key() {
    let derived = RoomCrypto::open(ROOM, "hunter2").unwrap();
    let restored = RoomCrypto::restore(&derived.key_material()).unwrap();
    assert_eq!(derived.safety_number(), restored.safety_number());
    let wire = restored.encrypt("回到房间".as_bytes(), AAD).unwrap();
    assert_eq!(
        String::from_utf8(derived.decrypt(&wire, AAD).unwrap()).unwrap(),
        "回到房间"
    );
}

#[test]
fn restore_rejects_wrong_key_size() {
    assert!(RoomCrypto::restore(&[0u8; 31]).is_err());
    assert!(RoomCrypto::restore(&[0u8; 33]).is_err());
    assert!(RoomCrypto::open("   ", "").is_err());
}

#[test]
fn safety_number_matches_independent_sha256() {
    let key =
        hex::decode("0c507ff2ecd664e9c25d92bc01ab781e257e2a85c7f72b9b911ccdbd1513e16e").unwrap();
    assert_eq!(safety_number(&key), "157 030 228 012 167 001");
    assert_eq!(
        RoomCrypto::open(ROOM, "hunter2").unwrap().safety_number(),
        "157 030 228 012 167 001"
    );
}

#[test]
fn message_ids_are_opaque_random_hex() {
    let ids: Vec<String> = (0..64).map(|_| random_mid()).collect();
    for id in &ids {
        assert_eq!(id.len(), 16, "{id}");
        assert!(
            id.chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "{id}"
        );
    }
    let unique: std::collections::HashSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len());
}

#[test]
fn topics_match_kotlin() {
    let hash = room_hash(ROOM);
    assert_eq!(clip_topic(&hash), "cs/v1/61232506/clip");
    assert_eq!(presence_topic(&hash, "d1"), "cs/v1/61232506/presence/d1");
    assert_eq!(sync_topic(&hash), "cs/v1/61232506/sync");
    assert_eq!(room_filter(&hash), "cs/v1/61232506/#");
    assert_eq!(client_id(&hash, "d1"), "cs-61232506-d1");
}

#[test]
fn topic_parsing_rejects_foreign_shapes() {
    let hash = room_hash(ROOM);
    assert_eq!(
        parse_topic(&clip_topic(&hash)),
        Some(clipsync_core::RoomTopic {
            room_hash: hash.clone(),
            channel: Channel::Clip,
            device_id: None
        })
    );
    assert_eq!(
        parse_topic(&presence_topic(&hash, "device-1"))
            .unwrap()
            .device_id
            .as_deref(),
        Some("device-1")
    );
    assert_eq!(
        parse_topic(&sync_topic(&hash)).unwrap().channel,
        Channel::Sync
    );
    assert_eq!(parse_topic(&format!("cs/v1/{hash}/unknown")), None);
    assert_eq!(parse_topic(&format!("other/{hash}/clip")), None);
    assert_eq!(parse_topic(&format!("cs/v1/{hash}/clip/extra")), None);
    assert_eq!(parse_topic(&format!("cs/v1/{hash}/presence")), None);
    assert_eq!(parse_topic("cs/v1"), None);
}

#[test]
fn presence_json_is_byte_identical_to_kotlin() {
    let wire = serde_json::to_string(&PresenceEnvelope {
        ct: "abc".into(),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(wire, "{\"v\":1,\"ct\":\"abc\"}");
    assert_eq!(
        serde_json::to_string(&PresencePayload {
            dev: "d1".into(),
            name: "Pixel".into(),
            os: "Android".into(),
            online: true,
            ts: 1700000000000,
            answer: false,
        })
        .unwrap(),
        "{\"dev\":\"d1\",\"name\":\"Pixel\",\"os\":\"Android\",\"online\":true,\"ts\":1700000000000,\"answer\":false}"
    );
    assert!(serde_json::from_str::<PresencePayload>(
        "{\"dev\":\"d1\",\"name\":\"Pixel\",\"os\":\"Android\",\"online\":true,\"ts\":1,\"answer\":true}"
    )
    .unwrap()
    .answer);
    assert!(
        !serde_json::from_str::<PresencePayload>(
            "{\"dev\":\"d1\",\"name\":\"Pixel\",\"os\":\"Android\",\"online\":true,\"ts\":1}"
        )
        .unwrap()
        .answer
    );
}

#[test]
fn clip_json_is_byte_identical_to_kotlin() {
    let wire = serde_json::to_string(&ClipEnvelope {
        mid: "261a17dceecbce2d".into(),
        ts: 1700000000000,
        dev: "d1".into(),
        name: "Pixel".into(),
        kind: "text".into(),
        ct: "abc".into(),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        wire,
        "{\"v\":1,\"mid\":\"261a17dceecbce2d\",\"ts\":1700000000000,\"dev\":\"d1\",\"name\":\"Pixel\",\"kind\":\"text\",\"ct\":\"abc\"}"
    );
    assert_eq!(
        serde_json::to_string(&ClipPayload {
            text: "hello clipsync".into(),
            len: 15
        })
        .unwrap(),
        "{\"text\":\"hello clipsync\",\"len\":15}"
    );
}

#[test]
fn file_json_is_byte_identical_to_kotlin() {
    let wire = serde_json::to_string(&FilePayload {
        name: "photo.png".into(),
        mime: "image/png".into(),
        size: 42,
        sha256: "ab".into(),
        link: "https://litterbox.catbox.moe/x.bin".into(),
        expires_at: 1700000000000,
    })
    .unwrap();
    assert_eq!(
        wire,
        "{\"name\":\"photo.png\",\"mime\":\"image/png\",\"size\":42,\"sha256\":\"ab\",\"link\":\"https://litterbox.catbox.moe/x.bin\",\"expiresAt\":1700000000000}"
    );
    let parsed: FilePayload = serde_json::from_str(&wire).unwrap();
    assert_eq!(parsed.expires_at, 1700000000000);
}

#[test]
fn kotlin_json_decodes_into_rust_structs() {
    let envelope: ClipEnvelope = serde_json::from_str(
        "{\"v\":1,\"mid\":\"m\",\"ts\":1,\"dev\":\"d\",\"name\":\"n\",\"kind\":\"text\",\"ct\":\"c\"}",
    )
    .unwrap();
    assert_eq!(envelope.mid, "m");
    let presence: PresenceEnvelope = serde_json::from_str("{\"v\":1,\"ct\":\"c\"}").unwrap();
    assert_eq!(presence.ct, "c");
    let no_v: ClipEnvelope = serde_json::from_str(
        "{\"mid\":\"m\",\"ts\":1,\"dev\":\"d\",\"name\":\"n\",\"kind\":\"text\",\"ct\":\"c\"}",
    )
    .unwrap();
    assert_eq!(no_v.v, 1);
}
