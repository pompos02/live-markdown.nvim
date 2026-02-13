mod nvim;

pub mod plugin;
pub mod protocol;
pub mod render;
pub mod server;
pub mod session;

#[nvim_oxi::plugin]
fn markdown_render_native() -> nvim_oxi::Result<nvim_oxi::Dictionary> {
    nvim::module()
}
