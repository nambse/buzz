use super::*;

#[test]
fn stage_bounds_invalid_ciphertext_and_invalid_text_are_closed() {
    let f = Fixture::new();
    let outer = f.wrap(f.rumor("bounds").as_json().as_bytes());
    let key = DmDecryptKey::for_recipient(&f.employee, f.employee.public_key()).unwrap();
    assert_eq!(
        decode(&key, &f.expected(&outer), &vec![b' '; MAX_OUTER_BYTES + 1]).err(),
        Some(DecodeError::Bounds)
    );
    f.refused_rumor(&vec![b' '; MAX_RUMOR_BYTES + 1], DecodeError::Bounds);
    assert_eq!(
        f.decode(&f.wrap_seal(&vec![b' '; MAX_SEAL_BYTES + 1], f.employee.public_key()))
            .err(),
        Some(DecodeError::Bounds)
    );
    for text in [
        " ".to_owned(),
        "nul\0canary".to_owned(),
        "x".repeat(MAX_TEXT_BYTES + 1),
    ] {
        f.refused_rumor(f.rumor(&text).as_json().as_bytes(), DecodeError::Text);
    }
    assert_eq!(
        f.decode(&f.wrap_seal(b"not-json", f.employee.public_key()))
            .err(),
        Some(DecodeError::Encoding)
    );
    f.refused_rumor(&[0xff], DecodeError::Encoding);
    let bad_ciphertext = EventBuilder::new(Kind::GiftWrap, "Ag==")
        .tags([Tag::public_key(f.employee.public_key())])
        .sign_with_keys(&f.other)
        .unwrap();
    assert_eq!(
        f.decode(&bad_ciphertext).err(),
        Some(DecodeError::Decryption)
    );
    let mut tampered = outer.content.into_bytes();
    tampered[20] = if tampered[20] == b'A' { b'B' } else { b'A' };
    let tampered = EventBuilder::new(Kind::GiftWrap, String::from_utf8(tampered).unwrap())
        .tags([Tag::public_key(f.employee.public_key())])
        .sign_with_keys(&f.other)
        .unwrap();
    assert_eq!(f.decode(&tampered).err(), Some(DecodeError::Decryption));
}

#[test]
fn optional_reply_is_verified_as_a_claim_only_and_other_tags_are_refused() {
    let f = Fixture::new();
    let reply = "aa".repeat(32);
    for tag in [
        vec!["e".to_owned(), reply.clone()],
        vec![
            "e".to_owned(),
            reply.clone(),
            "".to_owned(),
            "reply".to_owned(),
        ],
    ] {
        let mut rumor = f.rumor("a reply");
        let mut tags = rumor.tags.to_vec();
        tags.push(Tag::parse(tag).unwrap());
        rumor.tags = tags.into_iter().collect();
        rumor.id = None;
        rumor.ensure_id();
        let result = f.decode(&f.wrap(rumor.as_json().as_bytes())).unwrap();
        assert_eq!(result.reply_to(), Some(EventId::from_hex(&reply).unwrap()));
    }
    for extra in [
        json!(["h", "private-channel"]),
        json!(["e", reply, "", "root"]),
        json!(["e", reply, "https://unselected.invalid", "reply"]),
    ] {
        let mut rumor = serde_json::to_value(f.rumor("forged tags")).unwrap();
        rumor["tags"].as_array_mut().unwrap().push(extra);
        f.refused_rumor(&serde_json::to_vec(&rumor).unwrap(), DecodeError::Tags);
    }
}
