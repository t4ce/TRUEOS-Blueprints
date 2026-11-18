//! Finite load check of the native Spirit snapshot and label path.

use std::{io::Write, thread, time::Duration};

use trueos::spirit::{
    self,
    spirit_agents::{AgentEntry, AgentLifecycle, AgentRuntime, AgentSnapshot, SNAPSHOT_VERSION},
};

const RUN_HOLD: Duration = Duration::from_secs(26);
const REST_HOLD: Duration = Duration::from_secs(3);

fn running_agent(index: usize) -> AgentEntry {
    AgentEntry {
        id: format!("spirit-labels-probe-child-{index:02}"),
        parent_id: Some("spirit-labels-probe-root".into()),
        name: Some("/probe/check".into()),
        nickname: Some(format!("Probe {index:02}")),
        role: Some("visual-probe".into()),
        lifecycle: Some(AgentLifecycle::Running),
        runtime: Some(AgentRuntime::Active),
        waiting_approval: false,
        waiting_input: false,
        loaded: true,
    }
}

fn publish(snapshot: &mut AgentSnapshot, stage: &str, hold: Duration) -> bool {
    snapshot.revision += 1;
    let code = match spirit::publish_agent_snapshot(snapshot) {
        Ok(()) => 0,
        Err(error) => error.0,
    };
    println!(
        "spirit-labels-probe stage={stage} revision={} agents={} hold_ms={} rc={code}",
        snapshot.revision,
        snapshot.agents.len(),
        hold.as_millis(),
    );
    let _ = std::io::stdout().flush();
    if code != 0 {
        return false;
    }

    if !hold.is_zero() {
        thread::sleep(hold);
    }
    true
}

fn run(snapshot: &mut AgentSnapshot) -> bool {
    snapshot.agents.extend((1..=10).map(running_agent));
    if !publish(snapshot, "running-10", RUN_HOLD) {
        return false;
    }

    snapshot.agents.extend((11..=16).map(running_agent));
    if !publish(snapshot, "running-16", RUN_HOLD) {
        return false;
    }

    for agent in &mut snapshot.agents {
        agent.waiting_approval = true;
    }
    if !publish(snapshot, "approval-16", REST_HOLD) {
        return false;
    }

    for agent in &mut snapshot.agents {
        agent.waiting_approval = false;
        agent.lifecycle = Some(AgentLifecycle::Completed);
        agent.runtime = Some(AgentRuntime::Idle);
    }
    publish(snapshot, "completed-16", REST_HOLD)
}

fn main() {
    println!("spirit-labels-probe begin wire_version={SNAPSHOT_VERSION}");
    let mut snapshot = AgentSnapshot {
        version: SNAPSHOT_VERSION,
        epoch: 1,
        revision: 0,
        complete: true,
        connected: true,
        agents: Vec::new(),
    };

    let accepted = run(&mut snapshot);

    // Clear this owner's markers even if a load stage was rejected.
    snapshot.connected = false;
    snapshot.agents.clear();
    let cleaned = publish(&mut snapshot, "disconnect", Duration::ZERO);
    println!("spirit-labels-probe end accepted={accepted} cleaned={cleaned}");
}
