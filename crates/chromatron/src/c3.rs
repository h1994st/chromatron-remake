//! Chromatron 3's data, decoded from the original binary at build time (build.rs).

include!(concat!(env!("OUT_DIR"), "/chromatron3_data.rs"));
