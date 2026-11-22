#![no_std]

#[cfg(test)]
mod tests;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    Vec,
};

const SCHEMA_VERSION: u32 = 1;
const RECORD_MIN_TTL: u32 = 3_110_400;
const RECORD_MAX_TTL: u32 = 6_220_800;
const MAX_NODES: u32 = 32;
const MAX_EDGES: u32 = 128;
const MAX_DURATION_SECONDS: u64 = 31_536_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContractError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidRole = 4,
    PlanExists = 5,
    PlanNotFound = 6,
    InvalidBounds = 7,
    InvalidDeadline = 8,
    DuplicateNode = 9,
    UnknownNode = 10,
    SelfLoop = 11,
    DuplicateEdge = 12,
    CycleDetected = 13,
    InvalidDuration = 14,
    VersionMismatch = 15,
    LimitReached = 16,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Planner = 1,
    Participant = 2,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanInput {
    pub node_ids: Vec<BytesN<32>>,
    pub edges: Vec<(BytesN<32>, BytesN<32>)>,
    pub durations: Vec<u64>,
    pub resource_hashes: Vec<BytesN<32>>,
    pub deadline: u64,
    pub expected_version: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RiskAnalysis {
    pub tenant_id: BytesN<32>,
    pub project_id: BytesN<32>,
    pub plan_id: BytesN<32>,
    pub plan_hash: BytesN<32>,
    pub nodes_hash: BytesN<32>,
    pub edges_hash: BytesN<32>,
    pub durations_hash: BytesN<32>,
    pub resources_hash: BytesN<32>,
    pub node_count: u32,
    pub edge_count: u32,
    pub longest_path_seconds: u64,
    pub available_seconds: u64,
    pub critical_path_hash: BytesN<32>,
    pub critical_path_node_count: u32,
    pub blocked_count: u32,
    pub blocked_hash: BytesN<32>,
    pub impossible_count: u32,
    pub impossible_hash: BytesN<32>,
    pub impossible_flag: bool,
    pub resource_conflict_count: u32,
    pub resource_conflict_hash: BytesN<32>,
    pub resource_conflict_flag: bool,
    pub deadline_risk: bool,
    pub expected_version: u32,
    pub submitted_at: u64,
    pub analyzed_at: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Role(Address, BytesN<32>, BytesN<32>, Role),
    Analysis(BytesN<32>, BytesN<32>, BytesN<32>),
}

struct Computed {
    plan_hash: BytesN<32>,
    nodes_hash: BytesN<32>,
    edges_hash: BytesN<32>,
    durations_hash: BytesN<32>,
    resources_hash: BytesN<32>,
    node_count: u32,
    edge_count: u32,
    longest_path_seconds: u64,
    available_seconds: u64,
    critical_path_hash: BytesN<32>,
    critical_path_node_count: u32,
    blocked_count: u32,
    blocked_hash: BytesN<32>,
    impossible_count: u32,
    impossible_hash: BytesN<32>,
    impossible_flag: bool,
    resource_conflict_count: u32,
    resource_conflict_hash: BytesN<32>,
    resource_conflict_flag: bool,
    deadline_risk: bool,
}

#[contract]
pub struct CapstoneScheduleRiskContract;

#[contractimpl]
impl CapstoneScheduleRiskContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        Ok(())
    }

    pub fn set_role(
        env: Env,
        admin: Address,
        account: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        role: Role,
        active: bool,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let key = DataKey::Role(account.clone(), tenant_id.clone(), project_id.clone(), role);
        env.storage().persistent().set(&key, &active);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("SKR_ROLE"),),
            (tenant_id, project_id, account, role, active),
        );
        Ok(())
    }

    pub fn submit_plan(
        env: Env,
        planner: Address,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        plan_id: BytesN<32>,
        input: PlanInput,
    ) -> Result<RiskAnalysis, ContractError> {
        planner.require_auth();
        Self::require_role(&env, &planner, &tenant_id, &project_id, Role::Planner)?;
        let now = env.ledger().timestamp();
        let computed = Self::compute(&env, &input, now)?;
        let key = DataKey::Analysis(tenant_id.clone(), project_id.clone(), plan_id.clone());
        if let Some(existing) = env
            .storage()
            .persistent()
            .get::<_, RiskAnalysis>(&key)
        {
            if existing.plan_hash == computed.plan_hash {
                Self::extend_persistent(&env, &key);
                return Ok(existing);
            }
            if input.expected_version != existing.expected_version {
                return Err(ContractError::VersionMismatch);
            }
            return Err(ContractError::PlanExists);
        }
        if input.expected_version != 0 {
            return Err(ContractError::VersionMismatch);
        }
        let analysis = RiskAnalysis {
            tenant_id: tenant_id.clone(),
            project_id: project_id.clone(),
            plan_id: plan_id.clone(),
            plan_hash: computed.plan_hash,
            nodes_hash: computed.nodes_hash,
            edges_hash: computed.edges_hash,
            durations_hash: computed.durations_hash,
            resources_hash: computed.resources_hash,
            node_count: computed.node_count,
            edge_count: computed.edge_count,
            longest_path_seconds: computed.longest_path_seconds,
            available_seconds: computed.available_seconds,
            critical_path_hash: computed.critical_path_hash,
            critical_path_node_count: computed.critical_path_node_count,
            blocked_count: computed.blocked_count,
            blocked_hash: computed.blocked_hash,
            impossible_count: computed.impossible_count,
            impossible_hash: computed.impossible_hash,
            impossible_flag: computed.impossible_flag,
            resource_conflict_count: computed.resource_conflict_count,
            resource_conflict_hash: computed.resource_conflict_hash,
            resource_conflict_flag: computed.resource_conflict_flag,
            deadline_risk: computed.deadline_risk,
            expected_version: 1,
            submitted_at: now,
            analyzed_at: now,
        };
        env.storage().persistent().set(&key, &analysis);
        Self::extend_persistent(&env, &key);
        env.events().publish(
            (symbol_short!("SKR_PLAN"),),
            (tenant_id, project_id, plan_id, analysis.plan_hash.clone()),
        );
        Ok(analysis)
    }

    pub fn get_analysis(
        env: Env,
        tenant_id: BytesN<32>,
        project_id: BytesN<32>,
        plan_id: BytesN<32>,
    ) -> Result<RiskAnalysis, ContractError> {
        let key = DataKey::Analysis(tenant_id, project_id, plan_id);
        let analysis: RiskAnalysis = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(ContractError::PlanNotFound)?;
        Self::extend_persistent(&env, &key);
        Ok(analysis)
    }

    pub fn version(_env: Env) -> u32 {
        SCHEMA_VERSION
    }

    fn compute(env: &Env, input: &PlanInput, now: u64) -> Result<Computed, ContractError> {
        let node_count = input.node_ids.len();
        if node_count == 0 || node_count > MAX_NODES || input.edges.len() > MAX_EDGES {
            return Err(ContractError::InvalidBounds);
        }
        if input.durations.len() != node_count || input.resource_hashes.len() != node_count {
            return Err(ContractError::InvalidBounds);
        }
        if input.deadline <= now {
            return Err(ContractError::InvalidDeadline);
        }
        let available_seconds = input
            .deadline
            .checked_sub(now)
            .ok_or(ContractError::InvalidDeadline)?;
        let mut seen_nodes: Vec<BytesN<32>> = Vec::new(env);
        for i in 0..node_count {
            let node = input.node_ids.get(i).ok_or(ContractError::InvalidBounds)?;
            if seen_nodes.contains(node.clone()) {
                return Err(ContractError::DuplicateNode);
            }
            seen_nodes.push_back(node);
            let duration = input.durations.get(i).ok_or(ContractError::InvalidBounds)?;
            if duration == 0 || duration > MAX_DURATION_SECONDS {
                return Err(ContractError::InvalidDuration);
            }
        }

        let mut edge_pairs: Vec<(u32, u32)> = Vec::new(env);
        let mut out_edges: Vec<Vec<u32>> = Vec::new(env);
        let mut indegree: Vec<u32> = Vec::new(env);
        for _ in 0..node_count {
            out_edges.push_back(Vec::new(env));
            indegree.push_back(0);
        }
        for i in 0..input.edges.len() {
            let (from_id, to_id) = input.edges.get(i).ok_or(ContractError::InvalidBounds)?;
            if from_id == to_id {
                return Err(ContractError::SelfLoop);
            }
            let from = input
                .node_ids
                .first_index_of(from_id.clone())
                .ok_or(ContractError::UnknownNode)?;
            let to = input
                .node_ids
                .first_index_of(to_id.clone())
                .ok_or(ContractError::UnknownNode)?;
            if edge_pairs.contains((from, to)) {
                return Err(ContractError::DuplicateEdge);
            }
            edge_pairs.push_back((from, to));
            let mut outs = out_edges.get(from).ok_or(ContractError::UnknownNode)?;
            outs.push_back(to);
            out_edges.set(from, outs);
            let next = indegree
                .get(to)
                .ok_or(ContractError::UnknownNode)?
                .checked_add(1)
                .ok_or(ContractError::LimitReached)?;
            indegree.set(to, next);
        }

        let mut work_indegree = indegree.clone();
        let mut ready: Vec<u32> = Vec::new(env);
        for i in 0..node_count {
            if work_indegree.get(i) == Some(0) {
                ready.push_back(i);
            }
        }
        let mut topo: Vec<u32> = Vec::new(env);
        while !ready.is_empty() {
            let mut best_pos = 0u32;
            for pos in 1..ready.len() {
                let candidate = ready.get(pos).ok_or(ContractError::CycleDetected)?;
                let current = ready.get(best_pos).ok_or(ContractError::CycleDetected)?;
                if candidate < current {
                    best_pos = pos;
                }
            }
            let node = ready.get(best_pos).ok_or(ContractError::CycleDetected)?;
            ready.remove(best_pos);
            topo.push_back(node);
            let outs = out_edges.get(node).ok_or(ContractError::UnknownNode)?;
            for j in 0..outs.len() {
                let next = outs.get(j).ok_or(ContractError::UnknownNode)?;
                let reduced = work_indegree
                    .get(next)
                    .ok_or(ContractError::UnknownNode)?
                    .checked_sub(1)
                    .ok_or(ContractError::CycleDetected)?;
                work_indegree.set(next, reduced);
                if reduced == 0 {
                    ready.push_back(next);
                }
            }
        }
        if topo.len() != node_count {
            return Err(ContractError::CycleDetected);
        }

        let mut finish: Vec<u64> = Vec::new(env);
        for i in 0..node_count {
            finish.push_back(input.durations.get(i).ok_or(ContractError::InvalidBounds)?);
        }
        for i in 0..topo.len() {
            let node = topo.get(i).ok_or(ContractError::CycleDetected)?;
            let base = finish.get(node).ok_or(ContractError::UnknownNode)?;
            let outs = out_edges.get(node).ok_or(ContractError::UnknownNode)?;
            for j in 0..outs.len() {
                let next = outs.get(j).ok_or(ContractError::UnknownNode)?;
                let duration = input.durations.get(next).ok_or(ContractError::InvalidBounds)?;
                let candidate = base
                    .checked_add(duration)
                    .ok_or(ContractError::InvalidDuration)?;
                if candidate > finish.get(next).ok_or(ContractError::UnknownNode)? {
                    finish.set(next, candidate);
                }
            }
        }

        let mut longest_path_seconds = 0u64;
        let mut blocked_count = 0u32;
        let mut impossible_count = 0u32;
        let mut critical_count = 0u32;
        let mut blocked_flags: Vec<bool> = Vec::new(env);
        let mut impossible_flags: Vec<bool> = Vec::new(env);
        let mut critical_nodes: Vec<BytesN<32>> = Vec::new(env);
        let mut impossible_nodes: Vec<BytesN<32>> = Vec::new(env);
        for i in 0..node_count {
            let elapsed = finish.get(i).ok_or(ContractError::InvalidBounds)?;
            if elapsed > longest_path_seconds {
                longest_path_seconds = elapsed;
            }
            let blocked = indegree.get(i).ok_or(ContractError::InvalidBounds)? > 0;
            if blocked {
                blocked_count += 1;
            }
            blocked_flags.push_back(blocked);
            let impossible = elapsed > available_seconds;
            if impossible {
                impossible_count += 1;
                impossible_nodes.push_back(
                    input.node_ids.get(i).ok_or(ContractError::InvalidBounds)?,
                );
            }
            impossible_flags.push_back(impossible);
        }
        for i in 0..node_count {
            let elapsed = finish.get(i).ok_or(ContractError::InvalidBounds)?;
            if elapsed == longest_path_seconds {
                critical_count += 1;
                critical_nodes.push_back(
                    input.node_ids.get(i).ok_or(ContractError::InvalidBounds)?,
                );
            }
        }

        let mut conflicts: Vec<BytesN<32>> = Vec::new(env);
        for i in 0..node_count {
            let resource = input
                .resource_hashes
                .get(i)
                .ok_or(ContractError::InvalidBounds)?;
            if conflicts.contains(resource.clone()) {
                continue;
            }
            let mut occurrences = 0u32;
            for j in 0..node_count {
                if input
                    .resource_hashes
                    .get(j)
                    .ok_or(ContractError::InvalidBounds)?
                    == resource
                {
                    occurrences += 1;
                }
            }
            if occurrences > 1 {
                conflicts.push_back(resource);
            }
        }

        let nodes_hash = Self::hash_ids(env, &input.node_ids);
        let edges_hash = Self::hash_edges(env, &input.edges);
        let durations_hash = Self::hash_durations(env, &input.durations);
        let resources_hash = Self::hash_ids(env, &input.resource_hashes);
        let critical_path_hash = Self::hash_ids(env, &critical_nodes);
        let blocked_hash = Self::hash_flags(env, &blocked_flags);
        let impossible_hash = Self::hash_flags(env, &impossible_flags);
        let resource_conflict_hash = Self::hash_ids(env, &conflicts);
        let mut plan_bytes = Bytes::new(env);
        Self::append_id(&mut plan_bytes, &nodes_hash);
        Self::append_id(&mut plan_bytes, &edges_hash);
        Self::append_id(&mut plan_bytes, &durations_hash);
        Self::append_id(&mut plan_bytes, &resources_hash);
        Self::append_u64(&mut plan_bytes, input.deadline);
        let plan_hash = env.crypto().sha256(&plan_bytes).to_bytes();

        Ok(Computed {
            plan_hash,
            nodes_hash,
            edges_hash,
            durations_hash,
            resources_hash,
            node_count,
            edge_count: input.edges.len(),
            longest_path_seconds,
            available_seconds,
            critical_path_hash,
            critical_path_node_count: critical_count,
            blocked_count,
            blocked_hash,
            impossible_count,
            impossible_hash,
            impossible_flag: impossible_count > 0,
            resource_conflict_count: conflicts.len(),
            resource_conflict_hash,
            resource_conflict_flag: !conflicts.is_empty(),
            deadline_risk: longest_path_seconds > available_seconds,
        })
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(ContractError::NotInitialized)?;
        if &admin != caller {
            return Err(ContractError::Unauthorized);
        }
        caller.require_auth();
        Ok(())
    }

    fn require_role(
        env: &Env,
        account: &Address,
        tenant_id: &BytesN<32>,
        project_id: &BytesN<32>,
        role: Role,
    ) -> Result<(), ContractError> {
        let key = DataKey::Role(account.clone(), tenant_id.clone(), project_id.clone(), role);
        let active: bool = env.storage().persistent().get(&key).unwrap_or(false);
        if !active {
            return Err(ContractError::Unauthorized);
        }
        Self::extend_persistent(env, &key);
        Ok(())
    }

    fn append_id(buffer: &mut Bytes, id: &BytesN<32>) {
        buffer.extend_from_array(&id.to_array());
    }

    fn append_u64(buffer: &mut Bytes, value: u64) {
        buffer.extend_from_slice(&value.to_le_bytes());
    }

    fn hash_ids(env: &Env, ids: &Vec<BytesN<32>>) -> BytesN<32> {
        let mut buffer = Bytes::new(env);
        Self::append_u64(&mut buffer, ids.len() as u64);
        for i in 0..ids.len() {
            if let Some(id) = ids.get(i) {
                Self::append_id(&mut buffer, &id);
            }
        }
        env.crypto().sha256(&buffer).to_bytes()
    }

    fn hash_edges(env: &Env, edges: &Vec<(BytesN<32>, BytesN<32>)>) -> BytesN<32> {
        let mut buffer = Bytes::new(env);
        Self::append_u64(&mut buffer, edges.len() as u64);
        for i in 0..edges.len() {
            if let Some((from, to)) = edges.get(i) {
                Self::append_id(&mut buffer, &from);
                Self::append_id(&mut buffer, &to);
            }
        }
        env.crypto().sha256(&buffer).to_bytes()
    }

    fn hash_durations(env: &Env, durations: &Vec<u64>) -> BytesN<32> {
        let mut buffer = Bytes::new(env);
        Self::append_u64(&mut buffer, durations.len() as u64);
        for i in 0..durations.len() {
            if let Some(duration) = durations.get(i) {
                Self::append_u64(&mut buffer, duration);
            }
        }
        env.crypto().sha256(&buffer).to_bytes()
    }

    fn hash_flags(env: &Env, flags: &Vec<bool>) -> BytesN<32> {
        let mut buffer = Bytes::new(env);
        Self::append_u64(&mut buffer, flags.len() as u64);
        for i in 0..flags.len() {
            buffer.push_back(if flags.get(i) == Some(true) { 1 } else { 0 });
        }
        env.crypto().sha256(&buffer).to_bytes()
    }

    fn extend_persistent(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, RECORD_MIN_TTL, RECORD_MAX_TTL);
    }
}
