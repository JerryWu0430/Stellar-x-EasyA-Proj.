#![no_std]
use core::{f32::consts::E, str};

use soroban_sdk::{
    contract, contractimpl, contractmeta, contracttype, token, Address, BytesN, ConversionError,
    Env, IntoVal, Map, Symbol, TryFromVal, Val, Vec,
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

#[contracttype]
#[derive(Clone)]
pub struct Project {
    pub id: u32,
    pub name: Symbol,
    pub description: Symbol,
    pub recipient: Address,
    pub started: u64,
    pub deadline: u64,
    pub target_amount: i128,
    pub current_amount: i128,
    pub data_points: Map<Symbol, DataPoint>,
    pub contributors_contribution_map: Map<Address, i128>,
    pub annotators_earning_map: Map<Address, i128>,
    pub state: State,
}

#[contracttype]
#[derive(Clone)]

pub enum DataKey {
    Project(u32),
    ProjectIDs,
    ProjectCount,
}

#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum State {
    Funding = 0,
    Annotating = 1,
    Success = 2,
    Expired = 3,
}

fn get_ledger_timestamp(e: &Env) -> u64 {
    e.ledger().timestamp()
}

fn get_recipient(e: &Env, project_id: u32) -> Address {
    return e
        .storage()
        .instance()
        .get::<_, Project>(&DataKey::Project(project_id))
        .unwrap()
        .recipient;
}

fn get_deadline(e: &Env, project_id: u32) -> u64 {
    return e
        .storage()
        .instance()
        .get::<_, Project>(&DataKey::Project(project_id))
        .unwrap()
        .deadline;
}

fn get_target_amount(e: &Env, project_id: u32) -> i128 {
    return e
        .storage()
        .instance()
        .get::<_, Project>(&DataKey::Project(project_id))
        .expect("not initialized")
        .target_amount;
}

fn get_user_deposited(e: &Env, adr: &Address, project_id: u32) -> i128 {
    let user_deposited = e
        .storage()
        .instance()
        .get::<_, Project>(&DataKey::Project(project_id))
        .unwrap()
        .contributors_contribution_map
        .get(adr.clone())
        .unwrap_or(0);
    return user_deposited;
}

fn get_balance(e: &Env, project_id: u32) -> i128 {
    return e
        .storage()
        .instance()
        .get::<_, Project>(&DataKey::Project(project_id))
        .unwrap()
        .current_amount;
}

fn target_reached(e: &Env, token_id: &Address, project_id: u32) -> bool {
    let target_amount = get_target_amount(e, project_id);
    let token_balance = get_balance(e, project_id);

    if token_balance >= target_amount {
        return true;
    };
    false
}

fn get_state(e: &Env, project_id: u32) -> State {
    let deadline = get_deadline(e, project_id);
    let token_id = e.current_contract_address();
    let current_timestamp = get_ledger_timestamp(e);

    let current_state = e
        .storage()
        .instance()
        .get::<_, Project>(&DataKey::Project((project_id)))
        .unwrap()
        .state;
    if (current_state == State::Expired) {
        return current_state;
    }
    if (current_state == State::Funding) {
        if target_reached(e, &token_id, project_id) {
            let mut project = e
                .storage()
                .instance()
                .get::<_, Project>(&DataKey::Project((project_id)))
                .unwrap();
            project.state = State::Annotating;
            e.storage()
                .instance()
                .set(&DataKey::Project(project_id), &project);
        };
        if current_timestamp > deadline {
            let mut project = e
                .storage()
                .instance()
                .get::<_, Project>(&DataKey::Project((project_id)))
                .unwrap();
            project.state = State::Expired;
            e.storage()
                .instance()
                .set(&DataKey::Project(project_id), &project);
        };
    }
    if (current_state == State::Annotating) {
        if get_balance(e, project_id.clone()) < 1 {
            let mut project = e
                .storage()
                .instance()
                .get::<_, Project>(&DataKey::Project((project_id)))
                .unwrap();
            project.state = State::Success;
            e.storage()
                .instance()
                .set(&DataKey::Project(project_id), &project);
        };
    }
    let mut project = e
        .storage()
        .instance()
        .get::<_, Project>(&DataKey::Project((project_id)))
        .unwrap();
    project.state
}

fn set_user_deposited(e: &Env, user: &Address, amount: &i128, project_id: u32) {
    let mut project = e
        .storage()
        .instance()
        .get::<_, Project>(&DataKey::Project((project_id)))
        .unwrap();
    let current_contributions = project
        .contributors_contribution_map
        .get(user.clone())
        .unwrap_or(0);
    project
        .contributors_contribution_map
        .set(user.clone(), current_contributions + amount);
    e.storage()
        .instance()
        .set(&DataKey::Project(project_id), &project);
}

// Transfer tokens from the contract to the recipient
fn transfer(e: &Env, to: &Address, amount: &i128) {
    let token_contract_id = e.current_contract_address();
    let client = token::Client::new(e, &token_contract_id);
    client.transfer(&e.current_contract_address(), to, amount);
}

// Metadata that is added on to the WASM custom section
contractmeta!(
    key = "Description",
    val = "DataAnnotate Contract that help CrowdFund and Data Annotate"
);

fn get_project_ids(e: Env) -> Vec<u32> {
    e.storage()
        .instance()
        .get::<_, Vec<u32>>(&DataKey::ProjectIDs)
        .unwrap()
}

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
        name: Symbol,
        description: Symbol,
    ) {
        let mut project_count: u32 = e
            .storage()
            .instance()
            .get::<_, u32>(&DataKey::ProjectCount)
            .unwrap_or(0);
        let id = project_count;
        project_count += 1;
        let mut data_points: Map<Symbol, DataPoint> = Map::new(&e);
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
        let contributors_contribution_map: Map<Address, i128> = Map::new(&e);
        let annotators_earnings_map: Map<Address, i128> = Map::new(&e);

        let project = Project {
            id: id,
            name: name,
            description: description,
            recipient: recipient,
            state: State::Funding,
            started: get_ledger_timestamp(&e),
            contributors_contribution_map: contributors_contribution_map,
            annotators_earning_map: annotators_earnings_map,
            deadline: deadline,
            target_amount: target_amount,
            current_amount: 0,
            data_points: data_points,
        };
        e.storage().instance().set(&DataKey::Project(id), &project);
        e.storage()
            .instance()
            .set(&DataKey::ProjectCount, &project_count);

        let mut project_ids: Vec<u32> = e
            .storage()
            .instance()
            .get::<_, Vec<u32>>(&DataKey::ProjectIDs)
            .unwrap_or(Vec::new(&e));
        project_ids.push_back(id);
        e.storage()
            .instance()
            .set(&DataKey::ProjectIDs, &project_ids);
    }

    pub fn get_projects(e: Env) -> Vec<Project> {
        let mut projects: Vec<Project> = Vec::new(&e);
        let project_ids = get_project_ids(e.clone());
        for project_id in project_ids.iter() {
            let project = e
                .storage()
                .instance()
                .get::<_, Project>(&DataKey::Project(project_id.clone()))
                .unwrap();
            projects.push_back(project);
        }
        projects
    }

    pub fn deadline(e: Env, project_id: u32) -> u64 {
        get_deadline(&e, project_id)
    }

    pub fn state(e: Env, project_id: u32) -> u32 {
        get_state(&e, project_id) as u32
    }

    pub fn target(e: Env, project_id: u32) -> i128 {
        get_target_amount(&e, project_id)
    }

    pub fn token(e: Env) -> Address {
        e.current_contract_address()
    }

    pub fn balance(e: Env, user: Address, project_id: u32) -> i128 {
        let recipient = get_recipient(&e, project_id);
        if get_state(&e, project_id) == State::Annotating {
            if user != recipient {
                return 0;
            };
            return get_balance(&e, project_id);
        };

        get_user_deposited(&e, &user, project_id)
    }

    pub fn contribute(e: Env, user: Address, amount: i128, project_id: u32) {
        user.require_auth();
        assert!(amount > 0, "amount must be positive");
        assert!(
            get_state(&e, project_id) == State::Funding,
            "sale is not running"
        );
        let token_id = e.current_contract_address();
        let current_target_met = target_reached(&e, &token_id, project_id);

        let balance = get_user_deposited(&e, &user, project_id);
        set_user_deposited(&e, &user, &(balance + amount), project_id);

        let client = token::Client::new(&e, &token_id);
        client.transfer(&user, &e.current_contract_address(), &amount);
        let mut project = e
            .storage()
            .instance()
            .get::<_, Project>(&DataKey::Project(project_id))
            .unwrap();
        let current_contributions = project
            .contributors_contribution_map
            .get(user.clone())
            .unwrap_or(0);
        project
            .contributors_contribution_map
            .set(user.clone(), current_contributions + &amount);
        e.storage()
            .instance()
            .set(&DataKey::Project(project_id), &project);

        let contract_balance = get_balance(&e, project_id);

        // emit events
        events::pledged_amount_changed(&e, contract_balance);
        if !current_target_met && target_reached(&e, &token_id, project_id) {
            // only emit the target reached event once on the pledge that triggers target to be met
            events::target_reached(&e, contract_balance, get_target_amount(&e, project_id));
        }
    }

    pub fn get_name(e: Env, project_id: u32) -> Symbol {
        let project = e
            .storage()
            .instance()
            .get::<_, Project>(&DataKey::Project(project_id))
            .unwrap();
        project.name
    }

    pub fn get_description(e: Env, project_id: u32) -> Symbol {
        let project = e
            .storage()
            .instance()
            .get::<_, Project>(&DataKey::Project(project_id))
            .unwrap();
        project.description
    }

    pub fn submit(
        e: Env,
        to: Address,
        data_point_cid: Symbol,
        posy: u32,
        posx: u32,
        width: u32,
        height: u32,
        label: Symbol,
        project_id: u32,
    ) {
        to.require_auth();
        let state = get_state(&e, project_id);

        match state {
            State::Funding => {
                panic!("sale is still running")
            }
            State::Annotating => {
                // Do some checks to make sure the user has annotated.

                assert!(label != Symbol::new(&e, ""), "label cannot be empty");
                let mut project = e
                    .storage()
                    .instance()
                    .get::<_, Project>(&DataKey::Project(project_id))
                    .unwrap();
                let mut data_point = project.data_points.get(data_point_cid.clone()).unwrap();
                data_point.annotated = true;
                data_point.annotations.push_back(Annotation {
                    annotator: to.clone(),
                    posx: posx,
                    posy: posy,
                    width: width,
                    height: height,
                    label: label,
                });

                project.data_points.set(data_point_cid, data_point);

                e.storage()
                    .instance()
                    .set(&DataKey::Project(project_id), &project);
                transfer(&e, &to, &1);
                // check balance after transfer and if it's 0, we change state.
                get_state(&e, project_id);
            }
            State::Success => {
                // Do some checks to make sure the user has annotated.

                let balance = get_user_deposited(&e, &to, project_id);
                set_user_deposited(&e, &to, &0, project_id);
                transfer(&e, &to, &balance);
                let token_id = e.current_contract_address();
                let contract_balance = get_balance(&e, project_id);
                events::pledged_amount_changed(&e, contract_balance);
            }
            State::Expired => {
                panic!("Withdraw, expired")
            }
        };
    }

    pub fn withdraw(e: Env, user: Address, project_id: u32) {
        assert!(get_state(&e, project_id) == State::Expired, "not expired");
        user.require_auth();
        let balance = get_user_deposited(&e, &user, project_id);
        set_user_deposited(&e, &user, &0, project_id);
        transfer(&e, &user, &balance);
    }
}
