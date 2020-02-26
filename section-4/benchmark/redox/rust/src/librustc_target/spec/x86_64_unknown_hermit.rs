use crate::spec::{LldFlavor, LinkerFlavor, Target, TargetResult};

pub fn target() -> TargetResult {
    let mut base = super::hermit_base::opts();
    base.cpu = "x86-64".to_string();
    base.max_atomic_width = Some(64);
    base.features = "+rdrnd,+rdseed".to_string();
    base.stack_probes = true;

    Ok(Target {
        llvm_target: "x86_64-unknown-hermit".to_string(),
        target_endian: "little".to_string(),
        target_pointer_width: "64".to_string(),
        target_c_int_width: "32".to_string(),
        data_layout: "e-m:e-i64:64-f80:128-n8:16:32:64-S128".to_string(),
        arch: "x86_64".to_string(),
        target_os: "hermit".to_string(),
        target_env: String::new(),
        target_vendor: "unknown".to_string(),
        linker_flavor: LinkerFlavor::Lld(LldFlavor::Ld),
        options: base,
    })
}
