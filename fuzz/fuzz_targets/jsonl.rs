#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| kget::fuzz_api::jsonl(data));
