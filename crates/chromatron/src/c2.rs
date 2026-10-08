//! Chromatron 2's data, decoded from the original binary at build time (build.rs).

include!(concat!(env!("OUT_DIR"), "/chromatron2_data.rs"));
