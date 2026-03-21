pub mod annotations;
pub mod comments;
pub mod diff;
pub mod matching;
pub mod optparse;
pub mod output;
pub mod scope;
pub mod watchfile;

include!(concat!(env!("OUT_DIR"), "/comment_tokens.rs"));
