//! Turns merged claims into the ledger with a trust status per claim.

use podling_types::{Claim, Ledger};

use crate::error::Result;
use crate::stage::Stage;

pub struct BuildLedger;

impl Stage for BuildLedger {
    const ID: &'static str = "ledger";
    const VERSION: u32 = 1;
    type Input = Vec<Claim>;
    type Output = Ledger;

    fn run(&self, input: &Vec<Claim>) -> Result<Ledger> {
        Ok(Ledger::from_claims(input.iter().cloned()))
    }
}
