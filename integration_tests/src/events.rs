use oisy_trade_types_internal::event::{Event, EventType};

pub struct OisyTradeEventAssert {
    events: Vec<EventType>,
}

impl OisyTradeEventAssert {
    pub fn new(events: impl IntoIterator<Item = Event>) -> Self {
        let events: Vec<_> = events.into_iter().map(|e| e.payload).collect();
        Self { events }
    }

    pub fn satisfy<F>(self, check: F) -> Self
    where
        F: Fn(&[EventType]),
    {
        let events = self.events;
        let debug_guard = scopeguard::guard((), |()| {
            eprintln!(
                "ERROR: assertion on OISY TRADE events failed. Events: {:?}",
                events
            )
        });
        check(&events);
        scopeguard::ScopeGuard::into_inner(debug_guard);
        Self { events }
    }
}
