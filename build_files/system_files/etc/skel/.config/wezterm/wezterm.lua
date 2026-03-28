-- =============================================================================
-- zOS Wezterm Configuration
-- Full terminal with tabs, splits, panes — no Zellij needed
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

-- --- Mouse (Shift bypasses app mouse capture for paste) ---
config.bypass_mouse_reporting_modifiers = "SHIFT"

-- --- Tab bar ---
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

-- --- Key bindings ---
-- All use Ctrl+Shift (never intercepted by Hyprland)
config.keys = {
	-- =======================================================================
	-- PANE MANAGEMENT (splits, navigate, resize, zoom)
	-- =======================================================================

	-- Split panes
	{ key = "r", mods = "ALT", action = wezterm.action.SplitHorizontal({ domain = "CurrentPaneDomain" }) },
	{ key = "d", mods = "ALT", action = wezterm.action.SplitVertical({ domain = "CurrentPaneDomain" }) },

	-- Navigate panes (arrow keys)
	{ key = "LeftArrow", mods = "ALT", action = wezterm.action.ActivatePaneDirection("Left") },
	{ key = "DownArrow", mods = "ALT", action = wezterm.action.ActivatePaneDirection("Down") },
	{ key = "UpArrow", mods = "ALT", action = wezterm.action.ActivatePaneDirection("Up") },
	{ key = "RightArrow", mods = "ALT", action = wezterm.action.ActivatePaneDirection("Right") },

	-- Navigate panes (vim-style)
	{ key = "h", mods = "ALT", action = wezterm.action.ActivatePaneDirection("Left") },
	{ key = "j", mods = "ALT", action = wezterm.action.ActivatePaneDirection("Down") },
	{ key = "k", mods = "ALT", action = wezterm.action.ActivatePaneDirection("Up") },
	{ key = "l", mods = "ALT", action = wezterm.action.ActivatePaneDirection("Right") },

	-- Resize panes (vim-style + Alt)
	{ key = "h", mods = "CTRL|SHIFT|ALT", action = wezterm.action.AdjustPaneSize({ "Left", 5 }) },
	{ key = "j", mods = "CTRL|SHIFT|ALT", action = wezterm.action.AdjustPaneSize({ "Down", 5 }) },
	{ key = "k", mods = "CTRL|SHIFT|ALT", action = wezterm.action.AdjustPaneSize({ "Up", 5 }) },
	{ key = "l", mods = "CTRL|SHIFT|ALT", action = wezterm.action.AdjustPaneSize({ "Right", 5 }) },

	-- Zoom pane (toggle fullscreen for one pane)
	{ key = "z", mods = "CTRL|SHIFT", action = wezterm.action.TogglePaneZoomState },

	-- =======================================================================
	-- TAB MANAGEMENT
	-- =======================================================================

	{ key = "t", mods = "CTRL|SHIFT", action = wezterm.action.SpawnTab("CurrentPaneDomain") },
	{ key = "t", mods = "CTRL", action = wezterm.action.SpawnTab("CurrentPaneDomain") },
	{ key = "w", mods = "CTRL|SHIFT", action = wezterm.action.CloseCurrentPane({ confirm = true }) },
	{ key = "w", mods = "CTRL", action = wezterm.action.CloseCurrentPane({ confirm = true }) },
	{ key = "Tab", mods = "CTRL", action = wezterm.action.ActivateTabRelative(1) },
	{ key = "Tab", mods = "CTRL|SHIFT", action = wezterm.action.ActivateTabRelative(-1) },

	-- Navigate tabs by number
	{ key = "1", mods = "CTRL|ALT", action = wezterm.action.ActivateTab(0) },
	{ key = "2", mods = "CTRL|ALT", action = wezterm.action.ActivateTab(1) },
	{ key = "3", mods = "CTRL|ALT", action = wezterm.action.ActivateTab(2) },
	{ key = "4", mods = "CTRL|ALT", action = wezterm.action.ActivateTab(3) },
	{ key = "5", mods = "CTRL|ALT", action = wezterm.action.ActivateTab(4) },
	{ key = "6", mods = "CTRL|ALT", action = wezterm.action.ActivateTab(5) },
	{ key = "7", mods = "CTRL|ALT", action = wezterm.action.ActivateTab(6) },
	{ key = "8", mods = "CTRL|ALT", action = wezterm.action.ActivateTab(7) },
	{ key = "9", mods = "CTRL|ALT", action = wezterm.action.ActivateTab(8) },

	-- =======================================================================
	-- COPY / PASTE
	-- =======================================================================

	{ key = "c", mods = "CTRL|SHIFT", action = wezterm.action.CopyTo("Clipboard") },
	{ key = "v", mods = "CTRL|SHIFT", action = wezterm.action.PasteFrom("Clipboard") },
	{ key = "v", mods = "CTRL", action = wezterm.action.PasteFrom("Clipboard") },

	-- =======================================================================
	-- OTHER
	-- =======================================================================

	{ key = "=", mods = "CTRL", action = wezterm.action.IncreaseFontSize },
	{ key = "-", mods = "CTRL", action = wezterm.action.DecreaseFontSize },
	{ key = "0", mods = "CTRL", action = wezterm.action.ResetFontSize },
	{ key = "Enter", mods = "CTRL|SHIFT", action = wezterm.action.ToggleFullScreen },
	{ key = "f", mods = "CTRL|SHIFT", action = wezterm.action.Search({ CaseInSensitiveString = "" }) },
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
