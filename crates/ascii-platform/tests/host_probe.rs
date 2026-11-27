#![cfg(windows)]

use ascii_platform::{GpuBackend, ObservationReason, ProbeStatus, probe_host};

#[test]
fn host_probe_is_read_only_and_leaves_interactive_gates_unrun() {
    let report = probe_host();
    println!("desktop hierarchy v4: {:#?}", report.hierarchy.summary);

    assert_eq!(report.strategy, "windows-shell-hierarchy-enumeration-v4");
    assert_eq!(report.environment.gpu_backend, GpuBackend::NotQueried);
    for gate in [
        report.gates.placement_below_icons,
        report.gates.focus_preservation,
        report.gates.win_d_behavior,
        report.gates.explorer_restart_recovery,
        report.gates.icon_interaction,
    ] {
        assert_eq!(gate.status, ProbeStatus::NotRun);
        assert_eq!(
            gate.reason,
            ObservationReason::RequiresInteractiveValidation
        );
        assert_eq!(gate.error, None);
    }
}
