//! Kiln daemon: background supervisor binary.
use kiln_core::state::ModeStateMachine;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Kiln daemon initializing...");
    let sm = ModeStateMachine::new();
    println!("Initial state: {:?}", sm.state());
    Ok(())
}
