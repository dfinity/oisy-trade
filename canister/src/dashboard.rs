use crate::state::State;
use askama::Template;
use candid::Principal;
use dex_types_internal::Mode;
use ic_stable_structures::Memory;

#[derive(Template)]
#[template(path = "dashboard.html")]
pub struct DashboardTemplate {
    pub canister_id: Principal,
    pub mode: String,
    pub total_events: u64,
    pub tokens: Vec<DashboardToken>,
}

pub struct DashboardToken {
    pub ledger_id: Principal,
    pub symbol: String,
    pub decimals: u8,
}

impl DashboardTemplate {
    pub fn from_state<MH: Memory, MB: Memory>(
        state: &State<MH, MB>,
        canister_id: Principal,
        total_events: u64,
    ) -> Self {
        let tokens = state
            .tokens()
            .iter()
            .map(|(token_id, metadata)| DashboardToken {
                ledger_id: *token_id.as_principal(),
                symbol: metadata.symbol.clone(),
                decimals: metadata.decimals,
            })
            .collect();
        Self {
            canister_id,
            mode: format_mode(state.mode()),
            total_events,
            tokens,
        }
    }
}

fn format_mode(mode: &Mode) -> String {
    match mode {
        Mode::GeneralAvailability => "GeneralAvailability".to_string(),
        Mode::RestrictedTo(principals) if principals.is_empty() => {
            "RestrictedTo: (none)".to_string()
        }
        Mode::RestrictedTo(principals) => {
            let list = principals
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!("RestrictedTo: {list}")
        }
    }
}
