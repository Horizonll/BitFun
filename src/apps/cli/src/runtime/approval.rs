#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CliApprovalPolicy {
    /// Inherit the persisted user interaction preference.
    Ask,
    /// Explicitly disable Auto mode for this invocation/session.
    DisableAuto,
    Reject,
    Auto,
}
