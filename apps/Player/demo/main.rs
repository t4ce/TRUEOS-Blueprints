mod data;

fn main() -> anyhow::Result<()> {
    player_scope::ui::run(data::config())
}
