//! Finite, visual-only check of the native Spirit snapshot and nickname path.
//! Uses production encoding and publishing, without model or input activity.

use std::{io::Write, thread, time::Duration};
use trueos::spirit::{self, spirit_agents::{
    AgentEntry, AgentLifecycle, AgentRuntime, AgentSnapshot, SNAPSHOT_VERSION,
}};

fn publish(snapshot: &AgentSnapshot, stage: &str) -> bool {
    let wire = spirit::spirit_agents::encode(snapshot).expect("valid probe snapshot");
    println!(
        "spirit-labels-probe wire_version={} stage={} wire={}",
        SNAPSHOT_VERSION, stage, String::from_utf8_lossy(&wire),
    );
    let code = match spirit::publish_agent_snapshot(snapshot) {
        Ok(()) => 0,
        Err(error) => error.0,
    };
    println!(
        "spirit-labels-probe wire_version={} stage={} revision={} rc={}",
        SNAPSHOT_VERSION, stage, snapshot.revision, code,
    );
    let _ = std::io::stdout().flush();
    code == 0
}

fn main() {
    println!("spirit-labels-probe begin expected_label=§Probe hold_ms=2000 stages=running,approval,completed,disconnect");
    let mut snapshot = AgentSnapshot {
        version: SNAPSHOT_VERSION,
        epoch: 1,
        revision: 1,
        complete: true,
        connected: true,
        agents: vec![AgentEntry {
            id: "spirit-labels-probe-child".into(),
            parent_id: Some("spirit-labels-probe-root".into()),
            name: Some("/probe/check".into()),
            nickname: Some("Probe".into()),
            role: Some("visual-probe".into()),
            lifecycle: Some(AgentLifecycle::Running),
            runtime: Some(AgentRuntime::Active),
            waiting_approval: false,
            waiting_input: false,
            loaded: true,
        }],
    };

    let mut accepted = publish(&snapshot, "running");
    if accepted {
        thread::sleep(Duration::from_secs(2));
        snapshot.revision += 1;
        snapshot.agents[0].waiting_approval = true;
        accepted = publish(&snapshot, "approval");
    }
    if accepted {
        thread::sleep(Duration::from_secs(2));
        snapshot.revision += 1;
        snapshot.agents[0].waiting_approval = false;
        snapshot.agents[0].lifecycle = Some(AgentLifecycle::Completed);
        snapshot.agents[0].runtime = Some(AgentRuntime::Idle);
        accepted = publish(&snapshot, "completed");
    }
    if accepted {
        thread::sleep(Duration::from_secs(2));
    }

    // Always attempt explicit cleanup, including after a rejected stage.
    snapshot.revision += 1;
    snapshot.connected = false;
    snapshot.agents.clear();
    let cleaned = publish(&snapshot, "disconnect");
    println!("spirit-labels-probe end accepted={} cleaned={}", accepted, cleaned);
}
