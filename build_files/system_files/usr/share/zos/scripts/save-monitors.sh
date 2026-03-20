#!/bin/bash
# Launch wdisplays, then save monitor layout to Hyprland config
wdisplays

# After wdisplays closes, save current monitor state
hyprctl monitors -j | python3 -c "
import json, sys
monitors = json.load(sys.stdin)
lines = ['# Monitor layout — saved by wdisplays']
for m in monitors:
    name = m['name']
    w = m['width']
    h = m['height']
    rate = m['refreshRate']
    x = m['x']
    y = m['y']
    scale = m['scale']
    lines.append(f'monitor={name},{w}x{h}@{rate:.0f},{x}x{y},{scale}')
print('\n'.join(lines))
" > ~/.config/hypr/monitors.conf

hyprctl reload
notify-send -t 3000 "Monitor Layout Saved" "Your monitor positions have been saved."
