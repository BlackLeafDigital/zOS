-- =============================================================================
-- zOS Wezterm Configuration
-- iTerm2-like experience: tabs, splits, GPU-accelerated
-- =============================================================================

local wezterm = require("wezterm")
local config = wezterm.config_builder()

-- --- Appearance ---
config.color_scheme = "Catppuccin Mocha"
config.font = wezterm.font_with_fallback({
	{ family = "JetBrains Mono", weight = "Regular" },
	{ family = "JetBrainsMono Nerd Font", weight = "Regular" },
	"Noto Color Emoji",
})
config.font_size = 12.0
config.line_height = 1.1

-- --- Window ---
config.window_background_opacity = 0.92
config.window_decorations = "RESIZE"
config.window_padding = {
	left = 12,
	right = 12,
	top = 8,
	bottom = 8,
}
config.initial_cols = 120
config.initial_rows = 36
config.enable_scroll_bar = false

-- --- Tab bar (iTerm2-like) ---
config.use_fancy_tab_bar = true
config.tab_bar_at_bottom = false
config.hide_tab_bar_if_only_one_tab = false
config.tab_max_width = 32
config.show_tab_index_in_tab_bar = true

-- --- Cursor ---
config.default_cursor_style = "SteadyBar"
config.cursor_blink_rate = 500

-- --- Scrollback ---
config.scrollback_lines = 10000

-- --- GPU ---
config.front_end = "WebGpu"
config.webgpu_power_preference = "HighPerformance"

-- --- Key bindings (iTerm2-inspired) ---
config.keys = {
	-- Split panes (like iTerm2)
	{ key = "d", mods = "SUPER", action = wezterm.action.SplitHorizontal({ domain = "CurrentPaneDomain" }) },
	{ key = "d", mods = "SUPER|SHIFT", action = wezterm.action.SplitVertical({ domain = "CurrentPaneDomain" }) },

	-- Navigate panes
	{ key = "[", mods = "SUPER|ALT", action = wezterm.action.ActivatePaneDirection("Prev") },
	{ key = "]", mods = "SUPER|ALT", action = wezterm.action.ActivatePaneDirection("Next") },
	{ key = "h", mods = "SUPER|ALT", action = wezterm.action.ActivatePaneDirection("Left") },
	{ key = "l", mods = "SUPER|ALT", action = wezterm.action.ActivatePaneDirection("Right") },
	{ key = "k", mods = "SUPER|ALT", action = wezterm.action.ActivatePaneDirection("Up") },
	{ key = "j", mods = "SUPER|ALT", action = wezterm.action.ActivatePaneDirection("Down") },

	-- Tabs
	{ key = "t", mods = "SUPER", action = wezterm.action.SpawnTab("CurrentPaneDomain") },
	{ key = "w", mods = "SUPER", action = wezterm.action.CloseCurrentPane({ confirm = true }) },

	-- Navigate tabs by number
	{ key = "1", mods = "SUPER", action = wezterm.action.ActivateTab(0) },
	{ key = "2", mods = "SUPER", action = wezterm.action.ActivateTab(1) },
	{ key = "3", mods = "SUPER", action = wezterm.action.ActivateTab(2) },
	{ key = "4", mods = "SUPER", action = wezterm.action.ActivateTab(3) },
	{ key = "5", mods = "SUPER", action = wezterm.action.ActivateTab(4) },
	{ key = "6", mods = "SUPER", action = wezterm.action.ActivateTab(5) },
	{ key = "7", mods = "SUPER", action = wezterm.action.ActivateTab(6) },
	{ key = "8", mods = "SUPER", action = wezterm.action.ActivateTab(7) },
	{ key = "9", mods = "SUPER", action = wezterm.action.ActivateTab(8) },

	-- Font size
	{ key = "=", mods = "SUPER", action = wezterm.action.IncreaseFontSize },
	{ key = "-", mods = "SUPER", action = wezterm.action.DecreaseFontSize },
	{ key = "0", mods = "SUPER", action = wezterm.action.ResetFontSize },

	-- Copy/paste
	{ key = "c", mods = "SUPER", action = wezterm.action.CopyTo("Clipboard") },
	{ key = "v", mods = "SUPER", action = wezterm.action.PasteFrom("Clipboard") },

	-- Fullscreen
	{ key = "Enter", mods = "SUPER", action = wezterm.action.ToggleFullScreen },

	-- Search
	{ key = "f", mods = "SUPER", action = wezterm.action.Search({ CaseInSensitiveString = "" }) },
}

-- --- Mouse ---
config.mouse_bindings = {
	-- Right-click paste
	{
		event = { Down = { streak = 1, button = "Right" } },
		mods = "NONE",
		action = wezterm.action.PasteFrom("Clipboard"),
	},
}

return config
