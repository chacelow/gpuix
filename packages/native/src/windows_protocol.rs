#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FrameKind {
    Animation,
    Resume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Token {
    pub generation: u64,
    serial: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FrameTicket {
    pub kind: FrameKind,
    pub token: Token,
    activation_generation: u64,
}

#[derive(Default)]
struct Listener {
    generation: u64,
    registered: bool,
}

pub(super) struct FrameProtocol {
    animation: Listener,
    resume: Listener,
    active: bool,
    alive: bool,
    serial: u64,
    activation_generation: u64,
    in_flight: Option<FrameTicket>,
    resume_pending: bool,
}

impl Default for FrameProtocol {
    fn default() -> Self {
        Self {
            animation: Listener::default(),
            resume: Listener::default(),
            active: false,
            alive: true,
            serial: 0,
            in_flight: None,
            resume_pending: false,
            activation_generation: 0,
        }
    }
}

impl FrameProtocol {
    fn listener(&self, kind: FrameKind) -> &Listener {
        match kind {
            FrameKind::Animation => &self.animation,
            FrameKind::Resume => &self.resume,
        }
    }

    pub fn replace(&mut self, kind: FrameKind, registered: bool) -> u64 {
        let listener = match kind {
            FrameKind::Animation => &mut self.animation,
            FrameKind::Resume => &mut self.resume,
        };
        listener.generation += 1;
        listener.registered = registered;
        let generation = listener.generation;
        if kind == FrameKind::Resume {
            self.resume_pending = false;
        }
        generation
    }

    pub fn current(&self, kind: FrameKind, generation: u64) -> bool {
        self.alive && self.listener(kind).generation == generation
    }

    pub fn set_active(&mut self, active: bool) {
        if self.active != active {
            self.activation_generation += 1;
            self.active = active;
        }
    }
    pub fn request_resume(&mut self) {
        if self.alive && self.resume.registered {
            self.resume_pending = true;
        }
    }

    pub fn defer_present(&self) -> bool {
        self.alive && self.resume.registered && (self.resume_pending
            || self.in_flight.is_some_and(|ticket| ticket.kind == FrameKind::Resume && self.valid(ticket)))
    }

    pub fn needs_animation(&self) -> bool {
        self.alive && self.active && self.animation.registered
            && self.in_flight.is_none() && !self.resume_pending
    }

    pub fn begin(&mut self, kind: FrameKind) -> Option<FrameTicket> {
        let wanted = match kind {
            FrameKind::Animation => self.needs_animation(),
            FrameKind::Resume => self.defer_present(),
        };
        if !wanted || self.in_flight.is_some() {
            return None;
        }
        self.serial += 1;
        let ticket = FrameTicket {
            kind,
            token: Token { generation: self.listener(kind).generation, serial: self.serial },
            activation_generation: self.activation_generation,
        };
        self.in_flight = Some(ticket);
        if kind == FrameKind::Resume {
            self.resume_pending = false;
        }
        Some(ticket)
    }

    pub fn valid(&self, ticket: FrameTicket) -> bool {
        self.alive && self.in_flight == Some(ticket)
            && self.listener(ticket.kind).registered
            && self.listener(ticket.kind).generation == ticket.token.generation
            && (ticket.kind == FrameKind::Resume
                || (self.active && ticket.activation_generation == self.activation_generation))
    }

    pub fn complete(&mut self, ticket: FrameTicket) -> bool {
        if self.in_flight != Some(ticket) {
            return false;
        }
        self.in_flight = None;
        true
    }

    pub fn shutdown(&mut self) {
        self.alive = false;
        self.resume_pending = false;
        self.animation.registered = false;
        self.resume.registered = false;
    }
}

#[derive(Default)]
pub(super) struct CloseProtocol {
    generation: u64,
    serial: u64,
    pending: Option<Token>,
    registered: bool,
}

impl CloseProtocol {
    pub fn replace(&mut self, registered: bool) -> u64 {
        self.generation += 1;
        self.registered = registered;
        self.generation
    }

    pub fn current(&self, generation: u64) -> bool {
        self.generation == generation && self.registered
    }

    pub fn registered(&self) -> bool { self.registered }

    pub fn pending_is_current(&self) -> bool {
        self.registered && self.pending.is_some_and(|token| token.generation == self.generation)
    }

    pub fn begin(&mut self) -> Option<Token> {
        if !self.registered || self.pending.is_some() {
            return None;
        }
        self.serial += 1;
        let token = Token { generation: self.generation, serial: self.serial };
        self.pending = Some(token);
        Some(token)
    }

    pub fn valid(&self, token: Token) -> bool {
        self.registered && self.pending == Some(token) && self.generation == token.generation
    }

    pub fn complete(&mut self, token: Token, allow: bool) -> bool {
        if self.pending != Some(token) { return false; }
        let allowed = allow && self.valid(token);
        self.pending = None;
        allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_and_replacement_cancel_but_keep_one_in_flight_until_ack() {
        let mut state = FrameProtocol::default();
        state.replace(FrameKind::Animation, true);
        state.set_active(true);
        let old = state.begin(FrameKind::Animation).unwrap();
        state.set_active(false);
        state.set_active(true);
        assert!(!state.valid(old));
        assert!(state.begin(FrameKind::Animation).is_none());
        assert!(state.complete(old));
        let current = state.begin(FrameKind::Animation).unwrap();
        assert!(!state.complete(old));
        assert!(state.valid(current));
        state.replace(FrameKind::Animation, false);
        assert!(!state.valid(current));
        assert!(state.complete(current));
        assert!(!state.needs_animation());
    }

    #[test]
    fn resume_defers_until_matching_completion_and_unregister_releases_gate() {
        let mut state = FrameProtocol::default();
        state.replace(FrameKind::Resume, true);
        state.request_resume();
        assert!(state.defer_present());
        let ticket = state.begin(FrameKind::Resume).unwrap();
        assert!(state.defer_present());
        assert!(state.begin(FrameKind::Resume).is_none());
        assert!(state.complete(ticket));
        assert!(!state.defer_present());
        state.request_resume();
        let stale = state.begin(FrameKind::Resume).unwrap();
        state.replace(FrameKind::Resume, false);
        assert!(!state.defer_present());
        assert!(!state.valid(stale));
        state.complete(stale);
        state.replace(FrameKind::Resume, true);
        state.request_resume();
        let closing = state.begin(FrameKind::Resume).unwrap();
        state.shutdown();
        assert!(!state.defer_present());
        assert!(!state.valid(closing));
        assert!(state.begin(FrameKind::Resume).is_none());
    }

    #[test]
    fn resume_waits_for_animation_ack_without_queuing_another_callback() {
        let mut state = FrameProtocol::default();
        state.replace(FrameKind::Animation, true);
        state.replace(FrameKind::Resume, true);
        state.set_active(true);
        let frame = state.begin(FrameKind::Animation).unwrap();
        state.request_resume();
        assert!(state.defer_present());
        assert!(state.begin(FrameKind::Resume).is_none());
        state.complete(frame);
        assert!(!state.needs_animation());
        assert!(state.begin(FrameKind::Resume).is_some());
    }

    #[test]
    fn close_coalesces_and_rejects_old_handlers_and_responses() {
        let mut state = CloseProtocol::default();
        state.replace(true);
        let stale = state.begin().unwrap();
        assert!(state.begin().is_none());
        state.replace(true);
        assert!(state.begin().is_none(), "replacement retains the bounded outstanding request");
        assert!(!state.valid(stale));
        assert!(!state.complete(stale, true));
        let current = state.begin().unwrap();
        assert!(!state.complete(stale, true));
        assert!(state.valid(current));
        assert!(!state.complete(current, false));
        let allowed = state.begin().unwrap();
        assert!(state.complete(allowed, true));
        assert!(!state.complete(allowed, true));
    }
}
