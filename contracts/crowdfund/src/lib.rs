#![no_std]
use core::{ str};

use soroban_sdk::{
    contract, contractimpl, contractmeta, contracttype, token, xdr::Asset, Address, BytesN, ConversionError, Env, IntoVal, Map, Symbol, TryFromVal, Val, Vec
};

mod events;
mod test;
mod testutils;

#[contracttype]
#[derive(Clone)]
pub struct Annotation {
    pub annotator: Address,
    pub posx: u32,
    pub posy: u32,
    pub width: u32,
    pub height: u32,
    pub label: Symbol,
}

#[contracttype]
#[derive(Clone)]
pub struct DataPoint {
    pub cid: Symbol,
    pub annotated: bool,
    pub annotations: Vec<Annotation>,
}



#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Deadline,
    Recipient,
    Started,
    Target,
    Token,
    User(Address),
    RecipientClaimed,
    DataPoints,
    ContributorsContributionMap,
    AnnotatorsEarningsMap,
    State
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum State {
    Funding = 0,
    Annotating = 1,
    Success = 2,
    Expired= 3,
}
impl IntoVal<Env, Val> for State {
    fn into_val(&self, env: &Env) -> Val {
        (*self as u32).into_val(env) // Convert the enum to its u32 representation for storage
    }
}

// Implement TryFromVal for State (convert Val -> State)
impl TryFromVal<Env, Val> for State {
    type Error = ConversionError; 
    fn try_from_val(env: &Env, val: &Val) -> Result<Self, soroban_sdk::ConversionError> {
        let state_num = u32::try_from_val(env, val)?;  // Convert Val to u32 first
        match state_num {
            0 => Ok(State::Funding),
            1 => Ok(State::Annotating),
            2 => Ok(State::Success),
            3 => Ok(State::Expired),
            _ => Err(ConversionError.into()),  // Handle unknown value error
        }
    }
}

fn get_ledger_timestamp(e: &Env) -> u64 {
    e.ledger().timestamp()
}

fn get_recipient(e: &Env) -> Address {
    e.storage()
        .instance()
        .get::<_, Address>(&DataKey::Recipient)
        .expect("not initialized")
}

fn get_deadline(e: &Env) -> u64 {
    e.storage()
        .instance()
        .get::<_, u64>(&DataKey::Deadline)
        .expect("not initialized")
}


fn get_target_amount(e: &Env) -> i128 {
    e.storage()
        .instance()
        .get::<_, i128>(&DataKey::Target)
        .expect("not initialized")
}

fn get_token(e: &Env) -> Address {
    e.storage()
        .instance()
        .get::<_, Address>(&DataKey::Token)
        .expect("not initialized")
}

fn get_user_deposited(e: &Env, user: &Address) -> i128 {
    e.storage()
        .instance()
        .get::<_, i128>(&DataKey::User(user.clone()))
        .unwrap_or(0)
}

fn get_balance(e: &Env, contract_id: &Address) -> i128 {
    let client = token::Client::new(e, contract_id);
    client.balance(&e.current_contract_address())
}

fn target_reached(e: &Env, token_id: &Address) -> bool {
    let target_amount = get_target_amount(e);
    let token_balance = get_balance(e, token_id);

    if token_balance >= target_amount {
        return true;
    };
    false
}


fn get_state(e: &Env) -> State {
    let deadline = get_deadline(e);
    let token_id = get_token(e);
    let current_timestamp = get_ledger_timestamp(e);
    let current_state = e.storage().instance().get::<_, State>(&DataKey::State).unwrap();
    if(current_state == State::Expired) {
        return current_state;
    }
    if(current_state == State::Funding) {
        if target_reached(e, &token_id) {
            e.storage().instance().set(&DataKey::State, &State::Annotating);
        };
        if current_timestamp > deadline {
            e.storage().instance().set(&DataKey::State, &State::Expired);
        };
    }
    if(current_state == State::Annotating) {
        if get_balance(e, &e.current_contract_address()) < 1  {
            e.storage().instance().set(&DataKey::State, &State::Success);
        };
    }
    return e.storage().instance().get::<_, State>(&DataKey::State).unwrap();
}

fn set_user_deposited(e: &Env, user: &Address, amount: &i128) {
    e.storage()
        .instance()
        .set(&DataKey::User(user.clone()), amount);
}


// Transfer tokens from the contract to the recipient
fn transfer(e: &Env, to: &Address, amount: &i128) {
    let token_contract_id = &get_token(e);
    let client = token::Client::new(e, token_contract_id);
    client.transfer(&e.current_contract_address(), to, amount);
}

// Metadata that is added on to the WASM custom section
contractmeta!(
    key = "Description",
    val = "Crowdfunding contract that allows users to deposit tokens and withdraw them if the target is not met"
);

#[contract]
struct DataAnnotate;

#[contractimpl]
#[allow(clippy::needless_pass_by_value)]
impl DataAnnotate {
    pub fn initialize(
        e: Env,
        recipient: Address,
        deadline: u64,
        target_amount: i128,
        data_point_cids: Vec<Symbol>,
    ) {
        assert!(
            !e.storage().instance().has(&DataKey::Recipient),
            "already initialized"
        );

        e.storage().instance().set(&DataKey::Recipient, &recipient);
        e.storage()
            .instance()
            .set(&DataKey::RecipientClaimed, &false);
        e.storage()
            .instance()
            .set(&DataKey::Started, &get_ledger_timestamp(&e));
        e.storage().instance().set(&DataKey::Deadline, &deadline);
        e.storage().instance().set(&DataKey::Target, &target_amount);
        e.storage().instance().set(&DataKey::Token, &e.current_contract_address());
        let mut data_points : Map<Symbol,DataPoint> = Map ::new(&e);
        for cid in data_point_cids.iter() {
            data_points.set(
                cid.clone(),
                DataPoint {
                    cid: cid.clone(), 
                    annotated: false,  
                    annotations: Vec::new(&e), 
                },
            );
        }
        e.storage().instance().set(&DataKey::DataPoints, &data_points);
        let contributors_contribution_map : Map<Address,i128>= Map::new(&e);
        e.storage().instance().set(&DataKey::ContributorsContributionMap, &contributors_contribution_map);
        let annotators_earnings_map : Map<Address,i128>= Map::new(&e);
        e.storage().instance().set(&DataKey::AnnotatorsEarningsMap, &annotators_earnings_map);
        e.storage().instance().set(&DataKey::State, &State::Funding);
    }

   
    pub fn deadline(e: Env) -> u64 {
        get_deadline(&e)
    }


    pub fn state(e: Env) -> u32 {
        get_state(&e) as u32
    }

    pub fn target(e: Env) -> i128 {
        get_target_amount(&e)
    }

    pub fn token(e: Env) -> Address {
        get_token(&e)
    }

    pub fn balance(e: Env, user: Address) -> i128 {
        let recipient = get_recipient(&e);
        if get_state(&e) == State::Annotating {
            if user != recipient {
                return 0;
            };
            return get_balance(&e, &get_token(&e));
        };

        get_user_deposited(&e, &user)
    }

    pub fn contribute(e: Env, user: Address, amount: i128) {
        user.require_auth();
        assert!(amount > 0, "amount must be positive");
        assert!(get_state(&e) == State::Funding, "sale is not running");
        let token_id = get_token(&e);
        let current_target_met = target_reached(&e, &token_id);

        let balance = get_user_deposited(&e, &user);
        set_user_deposited(&e, &user, &(balance + amount));
        
        let client = token::Client::new(&e, &token_id);
        client.transfer(&user, &e.current_contract_address(), &amount);
        let mut contributors_map = e.storage().instance().get::<_, Map<Address, i128>>(&DataKey::ContributorsContributionMap).unwrap();
        let current_contributions = contributors_map.get(user.clone()).unwrap_or(0);
        contributors_map.set(user.clone(), current_contributions + &amount);
        e.storage().instance().set(&DataKey::ContributorsContributionMap, &contributors_map);
                
        let contract_balance = get_balance(&e, &token_id);

        // emit events
        events::pledged_amount_changed(&e, contract_balance);
        if !current_target_met && target_reached(&e, &token_id) {
            // only emit the target reached event once on the pledge that triggers target to be met
            events::target_reached(&e, contract_balance, get_target_amount(&e));
        }
    }

    pub fn submit(e: Env, to: Address,  data_point_cid: Symbol, posy: u32, posx: u32, width: u32, height: u32, label: Symbol) {
        to.require_auth();
        let state = get_state(&e);

        match state {
            State::Funding => {
                panic!("sale is still running")
            }
            State::Annotating => {
                // Do some checks to make sure the user has annotated.
                
                assert!(label != Symbol::new(&e, ""), "label cannot be empty");

                let mut data_points = e.storage().instance().get::<_, Map<Symbol,DataPoint>>(&DataKey::DataPoints).unwrap();
                let mut data_point = data_points.get(data_point_cid.clone()).unwrap();
                data_point.annotated = true;
                data_point.annotations.push_back(
                    Annotation {
                    annotator: to.clone(),
                    posx: posx,
                    posy: posy,
                    width: width,
                    height: height,
                    label: label});

                data_points.set(data_point_cid, data_point);
                e.storage().instance().set(&DataKey::DataPoints, &data_points);
                transfer(&e, &to, &1);
                // check balance after transfer and if it's 0, we change state.

            }
            State::Success => {
                // Do some checks to make sure the user has annotated.
               
                
                let balance = get_user_deposited(&e, &to);
                set_user_deposited(&e, &to, &0);
                transfer(&e, &to, &balance);
                let token_id = get_token(&e);
                let contract_balance = get_balance(&e, &token_id);
                events::pledged_amount_changed(&e, contract_balance);
            }
            State::Expired => {
                panic!("Withdraw, expired")
            }
        };
    }

    pub fn withdraw(e: Env, user: Address) {
        assert!(get_state(&e) == State::Expired, "not expired");
        user.require_auth();
        let balance = get_user_deposited(&e, &user);
        set_user_deposited(&e, &user, &0);
        transfer(&e, &user, &balance);
    }
}