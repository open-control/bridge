use bytes::Bytes;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardDirection {
    ControllerToHost,
    HostToController,
}

pub enum GuardAction {
    Forward(Bytes),
    DropDuplicate,
}

#[derive(Default)]
struct DirectionGuardState {
    last_forwarded_ms: u64,
    last_payload: Option<Bytes>,
}

#[derive(Default)]
pub struct RelayGuard {
    enabled: bool,
    duplicate_window_ms: u64,
    controller_to_host: DirectionGuardState,
    host_to_controller: DirectionGuardState,
}

impl RelayGuard {
    pub fn new(enabled: bool, duplicate_window_ms: u64) -> Self {
        Self {
            enabled,
            duplicate_window_ms,
            ..Self::default()
        }
    }

    pub fn on_controller_message(&mut self, payload: Bytes, now_ms: u64) -> GuardAction {
        self.handle(GuardDirection::ControllerToHost, payload, now_ms)
    }

    pub fn on_host_message(&mut self, payload: Bytes, now_ms: u64) -> GuardAction {
        self.handle(GuardDirection::HostToController, payload, now_ms)
    }

    fn handle(&mut self, direction: GuardDirection, payload: Bytes, now_ms: u64) -> GuardAction {
        if !self.enabled {
            return GuardAction::Forward(payload);
        }

        let state = match direction {
            GuardDirection::ControllerToHost => &mut self.controller_to_host,
            GuardDirection::HostToController => &mut self.host_to_controller,
        };

        let is_duplicate = state
            .last_payload
            .as_ref()
            .map(|last| last == &payload)
            .unwrap_or(false)
            && now_ms.saturating_sub(state.last_forwarded_ms) < self.duplicate_window_ms;

        if is_duplicate {
            return GuardAction::DropDuplicate;
        }

        state.last_forwarded_ms = now_ms;
        state.last_payload = Some(payload.clone());
        GuardAction::Forward(payload)
    }
}
