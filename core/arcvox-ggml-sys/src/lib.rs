//! Raw bindings, generated at build time from the headers of the sources in
//! `vendor/`. Everything here is `unsafe`; `arcvox-inference` wraps the part
//! it uses.
#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case, dead_code)]
#![allow(clippy::all)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
