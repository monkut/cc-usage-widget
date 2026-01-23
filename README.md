# Claude Code Usage Widget

A desktop widget for Linux and macOS that displays real-time Claude Code usage statistics. Built with Tauri 2.0.

## Features

- Real-time usage tracking from local Claude Code data
- Automatic updates when usage changes (file watching)
- Glassmorphism UI with adjustable transparency
- Compact, always-visible display

## UI Components

### Title Bar
Custom draggable title bar with:
- **Refresh button** (↻) - Manually refresh usage data
- **Settings button** (gear icon) - Opens the settings panel
- **Minimize button** - Minimizes the window
- **Close button** - Closes the application

### Settings Panel
Collapsible panel with:
- **Transparency slider** (30-100%) - Adjusts window background opacity. Setting is persisted in localStorage.

### Quota Section
Two quota indicators displayed side by side:

#### Rolling 5hr Limit
- **Progress bar** - Visual representation of quota usage (green/yellow/red based on percentage)
- **Percentage** - Current usage as percentage of estimated limit
- **Message count** - Messages in window vs estimated limit (e.g., "150/225")

Note: This is an *estimate* based on message counts. Anthropic's actual quota calculation is more complex and may differ.

#### Weekly Limit
- **Progress bar** - Visual representation of weekly usage
- **Percentage** - Current week usage percentage
- **Reset date** - Shows next reset date (Sundays)
- **Plan name** - Your Claude subscription plan

### Activity Heatmap
GitHub-style contribution heatmap showing prompt activity over the last 12 weeks:
- **Grid layout** - 7 rows (days of week) × 12 columns (weeks)
- **Color intensity** - Darker green indicates more prompts that day
- **Tooltips** - Hover to see exact date and prompt count

### Models Section
Token usage breakdown by model:
- **Opus 4.5** (purple)
- **Sonnet 4** (blue)
- **Haiku 3.5** (green)

Each model shows the model name and total token count.

### Active Sessions
Scrollable list of Claude Code sessions active in the last 24 hours. Each row displays:
- **Directory** - Project directory path (truncated from start, full path in tooltip)
- **Model** - Current model in use (color-coded)
- **Context %** - Remaining context window percentage
- **Todos** - Number of active todo items (or "-" if none)
- **Duration** - How long the session has been active (e.g., "15m", "2h 30m")

### Last Updated
Timestamp showing when the data was last refreshed. Data auto-refreshes every 10 seconds and when Claude Code writes new data.

## Installation

### Prerequisites

**Linux (Ubuntu/Debian):**
```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev \
  libsoup-3.0-dev librsvg2-dev libgtk-3-dev libayatana-appindicator3-dev
```

**macOS:**
Xcode Command Line Tools required.

### Build

```bash
npm install
npm run tauri build
```

### Development

```bash
npm run tauri dev
```

## Data Source

Reads Claude Code JSONL files from:
- `~/.claude/projects/`
- `~/.config/claude/projects/`

No data is sent externally. All processing is local.

## License

MIT
