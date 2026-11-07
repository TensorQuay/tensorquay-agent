//! ext-check: conformance checks for extensions (ARCHITECTURE §15.1). Developer tooling, not a product: it runs a
//! third-party MCP server, template or decision engine against this project's own contract, reusing the product's
//! code, and never changes an install.
pub mod boot;
pub mod catalog;
pub mod cli;
pub mod decision;
pub mod mcp;
pub mod report;
pub mod run;

#[cfg(test)]
mod catalog_template_tests;
#[cfg(test)]
mod catalog_tests;
#[cfg(test)]
mod cli_decision_tests;
#[cfg(test)]
mod cli_tests;
#[cfg(test)]
mod report_tests;
