pub mod arrhenius;

pub use arrhenius::{
    MichaelisMentenConfig,
    michaelis_menten_rate,
    enzyme_ph_factor,
    enzyme_temp_factor,
    enzyme_activity_factor,
    enzyme_hydrolysis_rate,
    total_collagen_hydrolysis_rate,
    microbial_biomass_growth,
    EnzymeSimulationState,
    simulate_enzyme_degradation,
    DEFAULT_VMAX,
    DEFAULT_KM,
};
