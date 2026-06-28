use std::sync::OnceLock;

pub(crate) fn disable_grid() -> bool {
    env_flag("VARG_RENDER_DISABLE_GRID")
}

pub(crate) fn disable_csm_shadows() -> bool {
    env_flag("VARG_RENDER_DISABLE_CSM_SHADOWS")
}

pub(crate) fn disable_ssao() -> bool {
    env_flag("VARG_RENDER_DISABLE_SSAO")
}

pub(crate) fn disable_ssr() -> bool {
    env_flag("VARG_RENDER_DISABLE_SSR")
}

fn env_flag(name: &'static str) -> bool {
    static FLAGS: OnceLock<Vec<(&'static str, bool)>> = OnceLock::new();
    FLAGS
        .get_or_init(|| {
            [
                "VARG_RENDER_DISABLE_GRID",
                "VARG_RENDER_DISABLE_CSM_SHADOWS",
                "VARG_RENDER_DISABLE_SSAO",
                "VARG_RENDER_DISABLE_SSR",
            ]
            .into_iter()
            .map(|key| (key, read_env_flag(key)))
            .collect()
        })
        .iter()
        .find_map(|(key, value)| (*key == name).then_some(*value))
        .unwrap_or(false)
}

fn read_env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|value| {
            let value = value.trim();
            !value.is_empty()
                && value != "0"
                && !value.eq_ignore_ascii_case("false")
                && !value.eq_ignore_ascii_case("off")
                && !value.eq_ignore_ascii_case("no")
        })
        .unwrap_or(false)
}
