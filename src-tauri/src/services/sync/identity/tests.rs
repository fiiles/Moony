//! Known-answer vectors and refusals for device certificates and signed records (phase 5 spec
//! §5.3, §5.4). The vectors are the contract between devices: a change to these bytes needs a new
//! domain string and new vectors. They were generated once, checked field by field against the
//! layout of spec §5.3 (`field_offsets_match_the_spec`), cross-checked with Python's
//! `cryptography` Ed25519, and frozen.

use zeroize::Zeroizing;

use super::cert::{self, CertFields, CertKind, Certificate, Ticket, TicketSecret};
use super::record::{Record, RecordType};
use super::{sign_pub, SignSeed};
use crate::services::sync::hlc::DeviceId;

const VAULT: [u8; 16] = [0x11; 16];
const A: DeviceId = DeviceId([0xa1; 16]);
const B: DeviceId = DeviceId([0xa2; 16]);
const C: DeviceId = DeviceId([0xa3; 16]);
const AT: i64 = 1_794_816_000;
const TICKET_AT: i64 = 1_794_815_940;

fn seed(byte: u8) -> SignSeed {
    SignSeed::from_bytes(Zeroizing::new([byte; 32]))
}

fn fields(device: DeviceId, box_byte: u8) -> CertFields {
    CertFields {
        vault_id: VAULT,
        device,
        box_pub: [box_byte; 32],
        issued_at: AT,
    }
}

fn genesis_a() -> Certificate {
    cert::issue_self(CertKind::Genesis, &fields(A, 0xb1), &seed(0x01)).expect("genesis")
}

fn ticket_from_a() -> (TicketSecret, Ticket) {
    cert::ticket_from_secret(
        TicketSecret::from_bytes(Zeroizing::new([0x07; 32])),
        &seed(0x01),
        VAULT,
        A,
        TICKET_AT,
    )
}

fn paired_b() -> Certificate {
    let (secret, ticket) = ticket_from_a();
    cert::issue_paired(
        &secret,
        &ticket,
        A,
        &sign_pub(&seed(0x01)),
        &fields(B, 0xb2),
        &seed(0x02),
    )
    .expect("paired")
}

fn kit_b() -> Certificate {
    cert::issue_self(CertKind::RecoveryKit, &fields(B, 0xb2), &seed(0x02)).expect("kit")
}

fn rejoin_c() -> Certificate {
    cert::issue_rejoin(&seed(0x01), A, &fields(C, 0xb3), &seed(0x03)).expect("rejoin")
}

fn left_a() -> Record {
    Record::sign(RecordType::Left, VAULT, A, A, AT, 0, &seed(0x01)).expect("left")
}

fn removed_b_by_a() -> Record {
    Record::sign(RecordType::Removed, VAULT, B, A, AT, 3, &seed(0x01)).expect("removed")
}

fn confirmed_b_by_a() -> Record {
    Record::sign(RecordType::Confirmed, VAULT, B, A, AT, 3, &seed(0x01)).expect("confirmed")
}

fn all_certs() -> Vec<(&'static str, Certificate)> {
    vec![
        ("genesis", genesis_a()),
        ("paired", paired_b()),
        ("recovery kit", kit_b()),
        ("re-join", rejoin_c()),
    ]
}

// ── Vectors ─────────────────────────────────────────────────────────────────────────────────

const GENESIS_HEX: &str = "
    6d6f6f6e792064657669636520636572742076310111111111111111111111111111111111a1a1a1a1a1a1a1a1a1a1a1
    a1a1a1a1a18a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5cb1b1b1b1b1b1b1b1b1b1b1
    b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1000000006afab800a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a18a88e3
    dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c6db0a22d1ce05abcaa823f3a33ad24de0caca6
    ee7d59c07ce810bcaf675c336bdb6f0bb434cb00e7a557294ff0eb44dfc19ca2f724670eb86369dffcfb0f5d0727dac4
    109e69451349840a222bdfcf0a0809ce60a682c918fc02e4e80027fdd0bbe8622da768278f350aa27439012106fed9f9
    3bdb0acc8ddc2fd1cd03f42400
";
const PAIRED_HEX: &str = "
    6d6f6f6e792064657669636520636572742076310211111111111111111111111111111111a2a2a2a2a2a2a2a2a2a2a2
    a2a2a2a2a28139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394b2b2b2b2b2b2b2b2b2b2b2
    b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2000000006afab800a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a18a88e3
    dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5cea4a6c63e29c520abef5507b132ec5f9954776
    aebebe7b92421eea691446d22c000000006afab7c4722545ff7c51d9b2cd4e476042b96695ce1c0c8d3c16975bf94da2
    4c072b74d53837c63e5de5af63155bc4f8bcc62ea3ace5c83d017870b3cc0f4a1f2f3222017f4be4268e2a67a2f5d29a
    f6735e13bb65d017ea3b9dea48573cbc2e153f8c60c3d343a2a6b79483ff338234f7e67e712634aa076f79fe0ac6bd78
    73a2d23b08967102b08f53665718626514ff2e9ab8ab417c2c81ff28ec1b08bd4674c6b956b4523e037d28e7517c0d4d
    1fd156d81896e4cd4e5fafe21de592f967e26f5e04
";
const KIT_HEX: &str = "
    6d6f6f6e792064657669636520636572742076310311111111111111111111111111111111a2a2a2a2a2a2a2a2a2a2a2
    a2a2a2a2a28139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394b2b2b2b2b2b2b2b2b2b2b2
    b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2000000006afab800a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2813977
    0ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b3943707c7ff7b3b1867d1c831900e95c096ba9c99
    27a946670e1d5a9e0f170071214bf22d717e16c8a002502eea6472f80416748c18a72fac3a46e52ba3bbdc840563ffd8
    00a1814ff4489d8c2a7b4450af976752a9a2388cc1988f623d0970b50af1ab6c6e95c6670124e016c48d552c9ec67b1b
    7404849a35a4b9fbd5eb5e1204
";
const REJOIN_HEX: &str = "
    6d6f6f6e792064657669636520636572742076310411111111111111111111111111111111a3a3a3a3a3a3a3a3a3a3a3
    a3a3a3a3a3ed4928c628d1c2c6eae90338905995612959273a5c63f93636c14614ac8737d1b3b3b3b3b3b3b3b3b3b3b3
    b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3b3000000006afab800a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a18a88e3
    dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c3133d09c7ceaabb5a04dcd94890f5fc003326e
    67729614cdaafe7307b9993b89ab5bd81e7017ff8ae6a983d5ec6680a83baa9a04c6e490a43c5280ecc259b103ef9885
    b0d18c114dcf85753de9dddf0fec1b0f777149e52209f4e31dca35d9836620f27be0070b5a51d5b0c649532c1f80fa73
    49323932e27fdad89fec094e06
";
const LEFT_HEX: &str = "
    6d6f6f6e7920646576696365207265636f72642076310111111111111111111111111111111111a1a1a1a1a1a1a1a1a1
    a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1000000006afab80000000000371c66ca3f7b72f93dd2a247dc
    33473191322ac7403312a0f6ea848e6e41c9c32af0a1d51d52e77fa024e8bbaeb60cd04ebbb05cfbeee12f9d6a08c4c2
    26bf01
";
const REMOVED_HEX: &str = "
    6d6f6f6e7920646576696365207265636f72642076310211111111111111111111111111111111a2a2a2a2a2a2a2a2a2
    a2a2a2a2a2a2a2a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1000000006afab800000000030c937574223845b358c26432de
    23ae823321c08e3973e0c523536d8abccc1f132917d3545127341c18d840f2b709162fdb21c684498d1c9370339044ba
    93670e
";

/// A record of type 3 (the person confirmed a device, fix round of the trust store review).
const CONFIRMED_HEX: &str = "
    6d6f6f6e7920646576696365207265636f72642076310311111111111111111111111111111111a2a2a2a2a2a2a2a2a2
    a2a2a2a2a2a2a2a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1000000006afab80000000003efedb55a6153912bf53ebe5889
    1e5f50bd6ccda094c7aa03408494259d13fe6c6035cd962207bc932dd4fdd63818c501d552deb5aeb23e3f2e5a99e285
    35180f
";

fn squash(hex: &str) -> String {
    hex.split_whitespace().collect()
}

#[test]
fn certificates_match_the_frozen_vectors() {
    assert_eq!(hex::encode(genesis_a().encode()), squash(GENESIS_HEX));
    assert_eq!(hex::encode(paired_b().encode()), squash(PAIRED_HEX));
    assert_eq!(hex::encode(kit_b().encode()), squash(KIT_HEX));
    assert_eq!(hex::encode(rejoin_c().encode()), squash(REJOIN_HEX));
}

#[test]
fn records_match_the_frozen_vectors() {
    assert_eq!(hex::encode(left_a().encode()), squash(LEFT_HEX));
    assert_eq!(hex::encode(removed_b_by_a().encode()), squash(REMOVED_HEX));
    assert_eq!(
        hex::encode(confirmed_b_by_a().encode()),
        squash(CONFIRMED_HEX)
    );
}

#[test]
fn frozen_vectors_decode_and_verify() {
    for hex in [GENESIS_HEX, PAIRED_HEX, KIT_HEX, REJOIN_HEX] {
        let bytes = hex::decode(squash(hex)).expect("hex");
        let cert = Certificate::decode(&bytes).expect("decodes");
        cert.verify_signatures().expect("verifies");
    }
    let left = Record::decode(&hex::decode(squash(LEFT_HEX)).unwrap()).expect("left");
    left.verify(&sign_pub(&seed(0x01))).expect("left verifies");
    let removed = Record::decode(&hex::decode(squash(REMOVED_HEX)).unwrap()).expect("removed");
    removed
        .verify(&sign_pub(&seed(0x01)))
        .expect("removed verifies");
}

/// Every field sits where spec §5.3 and §5.4 say.
#[test]
fn field_offsets_match_the_spec() {
    let bytes = paired_b().encode();
    let domain = b"moony device cert v1";
    assert_eq!(&bytes[..20], domain);
    assert_eq!(bytes[20], 2, "kind");
    assert_eq!(&bytes[21..37], &VAULT, "vault_id");
    assert_eq!(&bytes[37..53], &B.0, "device_id");
    assert_eq!(&bytes[53..85], &sign_pub(&seed(0x02)), "sign_pub");
    assert_eq!(&bytes[85..117], &[0xb2; 32], "box_pub");
    assert_eq!(&bytes[117..125], &AT.to_be_bytes(), "issued_at");
    assert_eq!(&bytes[125..141], &A.0, "issuer_device");
    assert_eq!(&bytes[141..173], &sign_pub(&seed(0x01)), "issuer_sign_pub");
    let (secret, ticket) = ticket_from_a();
    assert_eq!(&bytes[173..205], &ticket.ticket_pub, "ticket_pub");
    assert_eq!(&bytes[173..205], &sign_pub(secret.as_seed()), "ticket_pub");
    assert_eq!(
        &bytes[205..213],
        &TICKET_AT.to_be_bytes(),
        "ticket_issued_at"
    );
    assert_eq!(&bytes[213..277], &ticket.holder_sig, "holder_sig");
    assert_eq!(bytes.len(), 277 + 64 + 64, "issuer_sig ‖ subject_sig");

    let genesis = genesis_a().encode();
    assert_eq!(genesis[20], 1);
    assert_eq!(&genesis[125..141], &A.0, "a genesis names itself as issuer");
    assert_eq!(genesis.len(), 173 + 128);

    let record = removed_b_by_a().encode();
    assert_eq!(&record[..22], b"moony device record v1");
    assert_eq!(record[22], 2, "type");
    assert_eq!(&record[23..39], &VAULT);
    assert_eq!(&record[39..55], &B.0, "subject");
    assert_eq!(&record[55..71], &A.0, "signer");
    assert_eq!(&record[71..79], &AT.to_be_bytes(), "at");
    assert_eq!(&record[79..83], &3u32.to_be_bytes(), "epoch");
    assert_eq!(record.len(), 83 + 64);
}

/// The signatures cover what spec §5.3 says, under the stated domain strings.
#[test]
fn signatures_cover_the_bytes_before_them() {
    use ed25519_dalek::{Signature, VerifyingKey};
    let verify = |public: &[u8; 32], message: &[u8], sig: &[u8]| {
        let key = VerifyingKey::from_bytes(public).expect("key");
        let sig = Signature::from_slice(sig).expect("sig");
        key.verify_strict(message, &sig).is_ok()
    };
    let bytes = paired_b().encode();
    let (secret, ticket) = ticket_from_a();
    let mut ticket_message = b"moony join ticket v1".to_vec();
    ticket_message.extend_from_slice(&VAULT);
    ticket_message.extend_from_slice(&A.0);
    ticket_message.extend_from_slice(&ticket.ticket_pub);
    ticket_message.extend_from_slice(&TICKET_AT.to_be_bytes());
    assert!(verify(
        &sign_pub(&seed(0x01)),
        &ticket_message,
        &ticket.holder_sig
    ));
    assert!(verify(
        &sign_pub(secret.as_seed()),
        &bytes[..277],
        &bytes[277..341]
    ));
    assert!(verify(
        &sign_pub(&seed(0x02)),
        &bytes[..341],
        &bytes[341..405]
    ));

    let record = left_a().encode();
    assert!(verify(&sign_pub(&seed(0x01)), &record[..83], &record[83..]));
}

// ── Round trips ─────────────────────────────────────────────────────────────────────────────

#[test]
fn every_kind_round_trips_and_verifies() {
    for (name, cert) in all_certs() {
        let decoded = Certificate::decode(&cert.encode()).expect(name);
        assert_eq!(decoded, cert, "{name}");
        decoded.verify_signatures().expect(name);
        let from_b64 = Certificate::from_b64(&cert.to_b64()).expect(name);
        assert_eq!(from_b64, cert, "{name}");
        assert_eq!(from_b64.hash(), cert.hash(), "{name}");
    }
    assert_eq!(paired_b().body.kind, CertKind::Paired);
    assert_eq!(rejoin_c().body.issuer_device, A);
    assert_eq!(rejoin_c().body.issuer_sign_pub, sign_pub(&seed(0x01)));
}

#[test]
fn records_round_trip() {
    for record in [left_a(), removed_b_by_a()] {
        let decoded = Record::decode(&record.encode()).expect("decode");
        assert_eq!(decoded, record);
        assert_eq!(Record::from_b64(&record.to_b64()).expect("b64"), record);
    }
}

#[test]
fn base64_is_url_safe_without_padding_and_strict() {
    let b64 = genesis_a().to_b64();
    assert!(!b64.contains('=') && !b64.contains('+') && !b64.contains('/'));
    assert!(Certificate::from_b64(&format!("{b64}=")).is_err());
    assert!(Certificate::from_b64("not base64!").is_err());
}

#[test]
fn hash_is_sha256_of_the_encoding() {
    use sha2::{Digest, Sha256};
    let cert = paired_b();
    let expected: [u8; 32] = Sha256::digest(cert.encode()).into();
    assert_eq!(cert.hash(), expected);
    assert_ne!(cert.hash(), kit_b().hash());
}

// ── Any flipped byte fails ──────────────────────────────────────────────────────────────────

#[test]
fn a_flipped_byte_anywhere_in_a_certificate_fails() {
    for (name, cert) in all_certs() {
        let bytes = cert.encode();
        for index in 0..bytes.len() {
            for mask in [0x01u8, 0x80] {
                let mut flipped = bytes.clone();
                flipped[index] ^= mask;
                let accepted = Certificate::decode(&flipped)
                    .and_then(|c| c.verify_signatures())
                    .is_ok();
                assert!(!accepted, "{name}: byte {index} ^ {mask:#x} was accepted");
            }
        }
    }
}

#[test]
fn a_flipped_byte_anywhere_in_a_record_fails() {
    let signer = sign_pub(&seed(0x01));
    for record in [left_a(), removed_b_by_a()] {
        let bytes = record.encode();
        for index in 0..bytes.len() {
            for mask in [0x01u8, 0x80] {
                let mut flipped = bytes.clone();
                flipped[index] ^= mask;
                let accepted = Record::decode(&flipped)
                    .and_then(|r| r.verify(&signer))
                    .is_ok();
                assert!(!accepted, "byte {index} ^ {mask:#x} was accepted");
            }
        }
    }
}

#[test]
fn truncated_or_extended_bytes_are_refused() {
    for (name, cert) in all_certs() {
        let bytes = cert.encode();
        assert!(
            Certificate::decode(&bytes[..bytes.len() - 1]).is_err(),
            "{name}"
        );
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(Certificate::decode(&longer).is_err(), "{name}");
    }
    assert!(Certificate::decode(&[]).is_err());
    let record = left_a().encode();
    assert!(Record::decode(&record[..record.len() - 1]).is_err());
    let mut longer = record.clone();
    longer.push(0);
    assert!(Record::decode(&longer).is_err());
}

// ── Per-kind rules ──────────────────────────────────────────────────────────────────────────

const BODY_LEN: usize = 173;
const TICKET_LEN: usize = 104;

#[test]
fn a_paired_certificate_without_a_ticket_is_refused() {
    let bytes = paired_b().encode();
    let mut cut = bytes[..BODY_LEN].to_vec();
    cut.extend_from_slice(&bytes[BODY_LEN + TICKET_LEN..]);
    assert!(Certificate::decode(&cut).is_err());
}

#[test]
fn other_kinds_with_a_ticket_are_refused() {
    let (_, ticket) = ticket_from_a();
    for cert in [genesis_a(), kit_b(), rejoin_c()] {
        let bytes = cert.encode();
        let mut padded = bytes[..BODY_LEN].to_vec();
        padded.extend_from_slice(&ticket.encode());
        padded.extend_from_slice(&bytes[BODY_LEN..]);
        assert!(Certificate::decode(&padded).is_err());
    }
}

/// Builds a certificate with the given signers, past every issuer's own checks: what a forger
/// could write into the synced column.
fn forge(
    body: cert::CertBody,
    ticket: Option<Ticket>,
    issuer: &SignSeed,
    subject: &SignSeed,
) -> Certificate {
    let mut cert = Certificate {
        body,
        ticket,
        issuer_sig: [0; 64],
        subject_sig: [0; 64],
    };
    cert.issuer_sig = issuer.sign(&cert.issuer_signed_bytes());
    cert.subject_sig = subject.sign(&cert.subject_signed_bytes());
    cert
}

#[test]
fn a_ticket_signed_by_another_key_fails() {
    // The ticket is signed by B's key, but the body names A as the holder.
    let (secret, ticket) = cert::ticket_from_secret(
        TicketSecret::from_bytes(Zeroizing::new([0x07; 32])),
        &seed(0x02),
        VAULT,
        A,
        TICKET_AT,
    );
    let mut body = paired_b().body;
    body.device = C;
    body.sign_pub = sign_pub(&seed(0x03));
    let forged = forge(body, Some(ticket), secret.as_seed(), &seed(0x03));
    assert!(forged.verify_signatures().is_err());
    assert!(Certificate::decode(&forged.encode())
        .and_then(|c| c.verify_signatures())
        .is_err());
}

#[test]
fn issue_paired_refuses_a_ticket_the_named_holder_did_not_sign() {
    let (secret, ticket) = cert::ticket_from_secret(
        TicketSecret::from_bytes(Zeroizing::new([0x07; 32])),
        &seed(0x02),
        VAULT,
        A,
        TICKET_AT,
    );
    let issued = cert::issue_paired(
        &secret,
        &ticket,
        A,
        &sign_pub(&seed(0x01)),
        &fields(C, 0xb3),
        &seed(0x03),
    );
    assert!(issued.is_err());
}

#[test]
fn a_paired_issuer_signature_by_another_key_than_the_ticket_fails() {
    let (_, ticket) = ticket_from_a();
    let body = paired_b().body;
    // Signed with the holder's own key instead of the ticket secret.
    let forged = forge(body, Some(ticket), &seed(0x01), &seed(0x02));
    assert!(forged.verify_signatures().is_err());
}

#[test]
fn a_self_issued_certificate_naming_another_issuer_fails() {
    for kind in [CertKind::Genesis, CertKind::RecoveryKit] {
        let mut body = genesis_a().body;
        body.kind = kind;
        body.issuer_device = B;
        assert!(forge(body.clone(), None, &seed(0x01), &seed(0x01))
            .verify_signatures()
            .is_err());
        let mut body = genesis_a().body;
        body.kind = kind;
        body.issuer_sign_pub = sign_pub(&seed(0x02));
        assert!(forge(body, None, &seed(0x02), &seed(0x01))
            .verify_signatures()
            .is_err());
    }
}

#[test]
fn a_subject_signature_by_another_key_fails() {
    for (name, cert) in all_certs() {
        let mut forged = cert.clone();
        forged.subject_sig = seed(0x09).sign(&forged.subject_signed_bytes());
        assert!(forged.verify_signatures().is_err(), "{name}");
    }
}

#[test]
fn a_rejoin_signed_by_another_than_the_named_predecessor_fails() {
    let body = rejoin_c().body;
    assert!(forge(body, None, &seed(0x02), &seed(0x03))
        .verify_signatures()
        .is_err());
}

#[test]
fn a_rejoin_or_pairing_naming_itself_as_issuer_fails() {
    let mut body = rejoin_c().body;
    body.issuer_device = C;
    body.issuer_sign_pub = sign_pub(&seed(0x03));
    assert!(forge(body, None, &seed(0x03), &seed(0x03))
        .verify_signatures()
        .is_err());
    assert!(cert::issue_rejoin(&seed(0x03), C, &fields(C, 0xb3), &seed(0x03)).is_err());
}

#[test]
fn issue_self_refuses_the_other_kinds() {
    assert!(cert::issue_self(CertKind::Paired, &fields(A, 0xb1), &seed(0x01)).is_err());
    assert!(cert::issue_self(CertKind::Rejoin, &fields(A, 0xb1), &seed(0x01)).is_err());
}

// ── Records ─────────────────────────────────────────────────────────────────────────────────

#[test]
fn a_left_record_verified_with_another_key_fails() {
    let record = left_a();
    assert!(record.verify(&sign_pub(&seed(0x02))).is_err());
    assert!(record.verify(&sign_pub(&seed(0x01))).is_ok());
}

#[test]
fn a_left_record_must_be_signed_by_its_subject() {
    assert!(Record::sign(RecordType::Left, VAULT, A, B, AT, 0, &seed(0x02)).is_err());
    let mut bytes = left_a().encode();
    bytes[55..71].copy_from_slice(&B.0);
    assert!(Record::decode(&bytes).is_err());
}

#[test]
fn a_device_cannot_sign_its_own_removal() {
    assert!(Record::sign(RecordType::Removed, VAULT, A, A, AT, 0, &seed(0x01)).is_err());
}

#[test]
fn a_device_cannot_confirm_itself_and_a_confirmation_verifies_with_its_signer() {
    assert!(Record::sign(RecordType::Confirmed, VAULT, A, A, AT, 0, &seed(0x01)).is_err());
    let record = confirmed_b_by_a();
    assert_eq!(record.kind, RecordType::Confirmed);
    record.verify(&sign_pub(&seed(0x01))).expect("by A");
    assert!(record.verify(&sign_pub(&seed(0x02))).is_err());
    assert_eq!(Record::from_b64(&record.to_b64()).expect("decodes"), record);
}

#[test]
fn record_fields_read_back() {
    let record = removed_b_by_a();
    assert_eq!(record.kind, RecordType::Removed);
    assert_eq!(record.vault_id, VAULT);
    assert_eq!(record.subject, B);
    assert_eq!(record.signer, A);
    assert_eq!(record.at, AT);
    assert_eq!(record.epoch, 3);
}

// ── Seeds ───────────────────────────────────────────────────────────────────────────────────

#[test]
fn secrets_print_redacted() {
    let seed = seed(0x01);
    assert_eq!(format!("{seed:?}"), "SignSeed(<redacted>)");
    let (secret, _) = ticket_from_a();
    assert_eq!(format!("{secret:?}"), "TicketSecret(<redacted>)");
}

#[test]
fn new_seeds_are_random_and_issue_tickets() {
    let a = super::new_sign_seed().expect("seed");
    let b = super::new_sign_seed().expect("seed");
    assert_ne!(a.expose(), b.expose());
    let (secret, ticket) = cert::issue_ticket(&a, VAULT, A, AT).expect("ticket");
    assert_eq!(ticket.ticket_pub, sign_pub(secret.as_seed()));
    assert_eq!(ticket.issued_at, AT);
}

/// Review of Task 5 (M2): a join ticket lives one hour. A paired certificate issued before its
/// ticket or more than an hour after it does not verify, and the issuer refuses to make one.
#[test]
fn a_paired_certificate_must_be_issued_within_an_hour_of_its_ticket() {
    let (secret, ticket) = ticket_from_a();
    let issue = |at: i64| {
        cert::issue_paired(
            &secret,
            &ticket,
            A,
            &sign_pub(&seed(0x01)),
            &CertFields {
                issued_at: at,
                ..fields(B, 0xb2)
            },
            &seed(0x02),
        )
    };
    assert!(issue(TICKET_AT).is_ok(), "at the ticket's time");
    assert!(
        issue(TICKET_AT + cert::TICKET_LIFETIME_SECS).is_ok(),
        "an hour on"
    );
    assert!(issue(TICKET_AT + cert::TICKET_LIFETIME_SECS + 1).is_err());
    assert!(issue(TICKET_AT - 1).is_err());

    // Forged past the issuer's check: verification refuses it on its own.
    for at in [TICKET_AT - 1, TICKET_AT + cert::TICKET_LIFETIME_SECS + 1] {
        let mut body = paired_b().body;
        body.issued_at = at;
        let late = forge(body, Some(ticket.clone()), secret.as_seed(), &seed(0x02));
        assert!(late.verify_signatures().is_err(), "issued at {at}");
    }
}
