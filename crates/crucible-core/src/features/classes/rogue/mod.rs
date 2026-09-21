//! Rogue class features (2024 rules), one file per feature; subclass
//! features live in a folder per subclass.

pub mod thief;

mod cunning_action;
mod cunning_strike;
mod reliable_talent;
mod sneak_attack;
mod steady_aim;

use crate::features::FeatureRegistry;
pub use cunning_action::CunningActionPlugin;
pub use cunning_strike::{
    cunning_strike_poison, CunningStrikePlugin, CunningStrikeTripPlugin,
    CunningStrikeWithdrawPlugin,
};
pub use reliable_talent::ReliableTalentPlugin;
pub use sneak_attack::SneakAttackPlugin;
pub use steady_aim::SteadyAimPlugin;

pub(super) fn register(registry: &mut FeatureRegistry) {
    sneak_attack::register(registry);
    cunning_action::register(registry);
    steady_aim::register(registry);
    cunning_strike::register(registry);
    reliable_talent::register(registry);
    thief::register(registry);
}
