//! NicaScan: directorio extensible de negocios de Nicaragua.
//!
//! El crate expone la lógica para que se pueda usar también como biblioteca en
//! un futuro servicio para agentes de IA.

pub mod cli;
pub mod config;
pub mod db;
pub mod export;
pub mod gazetteer;
pub mod http;
pub mod location;
pub mod model;
pub mod normalize;
pub mod output;
pub mod providers;
pub mod version;
