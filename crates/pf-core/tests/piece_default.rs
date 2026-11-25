//! `Piece::default()` : pièce neutre, valide une fois `id` et `name` fournis.

use pf_core::{Piece, PieceId, PieceLink, Project};

#[test]
fn piece_default_est_neutre_et_valide() {
    let d = Piece::default();
    assert_eq!(d.id, PieceId(0));
    assert!(d.name.is_empty() && d.outline.is_empty() && d.extra.is_empty());
    assert_eq!(d.quantity, 1);
    assert_eq!(d.link, PieceLink::Detached);
    assert!(!d.mirrored_pair);

    let mut p = Project::new("Essai");
    let id = PieceId(p.next_id);
    p.pieces.push(Piece {
        id,
        name: "Neutre".into(),
        ..Piece::default()
    });
    p.next_id += 1;
    p.validate().unwrap();
}
