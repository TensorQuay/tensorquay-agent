//! Decision engines (ADR-0013 point 6): external engines the gateway relays `decision/v1` to, configured like routes in
//! their own table.
mod document;
pub(crate) use document::{
    BudgetPeriod, EngineBudget, EngineDocument, EngineProtocol, valid_engine_name, validate,
};
