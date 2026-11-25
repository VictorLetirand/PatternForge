//! Design system « Precision Light » de PatternForge : jetons sémantiques,
//! thème egui, composants réutilisables et catalogue jouable (UI Lab).

pub mod components;
pub mod lab;
pub mod strings;
pub mod theme;
pub mod tokens;

#[cfg(test)]
mod tests;

pub use tokens::Tokens;
