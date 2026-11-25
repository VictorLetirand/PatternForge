//! D-04 : une clé en trop sur `punch` / `detached` est refusée ; lecture et écriture des anciennes formes inchangées.

use pf_core::{MarkingShape, PieceLink};

#[test]
fn d04_une_cle_en_trop_sur_detached_est_refusee() {
    let r: Result<PieceLink, _> = serde_json::from_str(r#"{"type": "detached", "extra": 1}"#);
    assert!(r.is_err(), "clé inconnue acceptée : {r:?}");
}

#[test]
fn d04_anciennes_formes_toujours_lues() {
    assert_eq!(
        serde_json::from_str::<PieceLink>(r#"{"type":"detached"}"#).unwrap(),
        PieceLink::Detached
    );
    assert_eq!(
        serde_json::from_str::<MarkingShape>(r#"{"type":"punch"}"#).unwrap(),
        MarkingShape::Punch
    );
}

#[test]
fn d04_ecriture_inchangee_octet_pour_octet() {
    assert_eq!(
        serde_json::to_string(&PieceLink::Detached).unwrap(),
        r#"{"type":"detached"}"#
    );
    assert_eq!(
        serde_json::to_string(&MarkingShape::Punch).unwrap(),
        r#"{"type":"punch"}"#
    );
    assert_eq!(
        serde_json::to_string_pretty(&MarkingShape::Punch).unwrap(),
        "{\n  \"type\": \"punch\"\n}"
    );
}

#[test]
fn d04_autres_variantes_gardent_leurs_champs_et_leur_refus() {
    assert_eq!(
        serde_json::from_str::<MarkingShape>(r#"{"type":"drill","diameter_mm":3.0}"#).unwrap(),
        MarkingShape::Drill { diameter_mm: 3.0 }
    );
    assert!(
        serde_json::from_str::<MarkingShape>(r#"{"type":"drill","diameter_mm":3.0,"z":1}"#)
            .is_err()
    );
    assert!(serde_json::from_str::<MarkingShape>(r#"{"type":"cross"}"#).is_err());
    assert!(serde_json::from_str::<PieceLink>(r#"{"type":"linked","sheet":1,"z":1}"#).is_err());
    assert!(serde_json::from_str::<PieceLink>(r#"{"type":"nope"}"#).is_err());
}
