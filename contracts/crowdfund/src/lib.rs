#![no_std]
use core::{i128, ops::Add};

use soroban_sdk::{
    contract, contractimpl, contractmeta, contracttype, token, Address, Env, IntoVal, String, Val,
    Vec,
};

mod events;
mod test;
mod testutils;

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    // Money
    Balance(i128),
    Target(i128),

    // List of contributors maps address to amount pledged
    Contributors(Vec<(Address, i128)>),

    // List of Annotators
    AnnotatorsCount(u32),
    Annotators(Vec<Address>),

    // Initiative parameters
    DataPoints(Vec<String>),
    Annotations(Vec<String>),

    // Funding status
    State(u32),

    // Timestamp of when the crowdfund started
    Started(u64),
}

fn get_ledger_timestamp(e: &Env) -> u64 {
    e.ledger().timestamp()
}

fn get_target_amount(e: &Env) -> i128 {
    e.storage()
        .instance()
        .get(&DataKey::Target)
        .expect("not initialized")
}

fn get_user_deposited(e: &Env, user: &Address) -> i128 {
    e.storage()
        .instance()
        .get(&DataKey::Contributors)
        .expect("not initialized")
        .iter()
        .find(|(addr, _)| addr == user)
        .map(|(_, amount)| amount)
        .unwrap_or(0)
}

fn get_balance(e: &Env, user: &Address) -> i128 {
    e.storage().get(&DataKey::Balance).expect("not initialized")
}

fn target_reached(e: &Env) -> bool {
    get_balance(e, &e.current_contract_address()) >= get_target_amount(e)
}

fn set_user_deposited(e: &Env, user: &Address, amount: &i128) {
    e.storage()
        .instance()
        .set(&DataKey::User(user.clone()), amount);
}

// Metadata that is added on to the WASM custom section
contractmeta!(
    key = "Description",
    val = "Crowdfunding contract that allows users to deposit tokens and contribute to "
);

#[contract]
struct Crowdfund;

#[contractimpl]
#[allow(clippy::needless_pass_by_value)]
impl Crowdfund {
    pub fn initialize(e: Env, target_amount: i128, data_points: u32) {
        e.storage()
            .instance()
            .set(&DataKey::Started, &get_ledger_timestamp(&e));
        e.storage().instance().set(&DataKey::Target, &target_amount);
        e.storage().instance().set(&DataKey::ContributorsCount, &0);
        e.storage()
            .instance()
            .set(&DataKey::DataPoints, &data_points);
    }

    pub fn started(e: Env) -> u64 {
        e.storage().get(&DataKey::Started).expect("not initialized")
    }

    pub fn target(e: Env) -> i128 {
        e.storage().get(&DataKey::Target).expect("not initialized")
    }

    pub fn balance(e: Env, user: Address) -> i128 {
        e.storage()
            .get(&DataKey::Balance(user))
            .expect("not initialized")
    }

    pub fn deposit(e: Env, user: Address, amount: i128) {
        user.require_auth();
        assert!(amount > 0, "amount must be positive");

        // Transfer the amount from the user to the contract
        let client = token::Client::new(&e, &token_id);
        client.transfer(&user, &e.current_contract_address(), &amount);

        // Update the balance
        let current_balance = get_balance(&e, &user);
        let new_balance = current_balance + amount;
        e.storage()
            .instance()
            .set(&DataKey::Balance(user), &new_balance);

        let contract_balance = get_balance(&e, &e.current_contract_address());
        // emit events
        events::pledged_amount_changed(&e, contract_balance);
        if target_reached(&e) {
            // only emit the target reached event once on the pledge that triggers target to be met
            events::target_reached(&e, contract_balance, get_target_amount(&e));
        }
    }

    pub fn withdraw(e: Env, to: Address) {
        let state = get_state(&e);
        let recipient = get_recipient(&e);

        match state {
            State::Running => {
                panic!("sale is still running")
            }
            State::Success => {
                assert!(
                    to == recipient,
                    "sale was successful, only the recipient may withdraw"
                );
                assert!(
                    !get_recipient_claimed(&e),
                    "sale was successful, recipient has withdrawn funds already"
                );

                // Directly transfer XLM to the recipient
                let contract_balance = get_balance(&e, &e.current_contract_address());
                e.ledger()
                    .transfer(&e.current_contract_address(), &recipient, &contract_balance);
                set_recipient_claimed(&e);
            }
            State::Expired => {
                assert!(
                    to != recipient,
                    "sale expired, the recipient may not withdraw"
                );

                // Withdraw full amount
                let balance = get_user_deposited(&e, &to);
                set_user_deposited(&e, &to, &0);
                e.ledger()
                    .transfer(&e.current_contract_address(), &to, &balance);

                // emit events
                let contract_balance = get_balance(&e, &e.current_contract_address());
                events::pledged_amount_changed(&e, contract_balance);
            }
        };
    }

    pub fn add_contributor(e: Env, contributor: Address) {
        let contributors_count = e
            .storage()
            .instance()
            .get::<_, u32>(&DataKey::ContributorsCount)
            .expect("Contributors count not initialized");
        let current_count = e
            .storage()
            .instance()
            .get::<_, u32>(&DataKey::Contributor(contributor.clone()))
            .unwrap_or(0);

        assert!(current_count == 0, "Contributor already added"); // Ensure contributor is not added twice

        // Store the contributor's address
        e.storage()
            .instance()
            .set(&DataKey::Contributor(contributor.clone()), &1); // Mark as added
    }

    pub fn payout_contributors(e: Env) {
        let budget = e
            .storage()
            .instance()
            .get::<_, i128>(&DataKey::Budget)
            .expect("Budget not initialized");
        let contributors_count = e
            .storage()
            .instance()
            .get::<_, u32>(&DataKey::ContributorsCount)
            .expect("Contributors count not initialized");
        let payout_per_contributor = budget / contributors_count as i128; // Calculate payout per contributor

        // Iterate over all contributors and transfer payouts
        for i in 0..contributors_count {
            let contributor_address = contributors[i as usize]; // Assuming `contributors` is a Vec<Address>
            let client = token::Client::new(&e, &get_token(&e));
            client.transfer(
                &e.current_contract_address(),
                &contributor_address,
                &payout_per_contributor,
            );
        }
    }
}

// Function to convert budget to float
fn budget_to_float(budget: i128) -> f64 {
    budget as f64 / 100.0 // Assuming budget is in cents
}

// Function to convert float to budget
fn float_to_budget(budget: f64) -> i128 {
    (budget * 100.0).round() as i128 // Convert to cents
}
