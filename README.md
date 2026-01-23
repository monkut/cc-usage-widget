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
- **Settings button** (gear icon) - Opens the settings panel
- **Minimize button** - Minimizes the window
- **Close button** - Closes the application

### Settings Panel
Collapsible panel with:
- **Transparency slider** (30-100%) - Adjusts window background opacity. Setting is persisted in localStorage.

### Period Selector
Filter usage statistics by time period:
- **Today** - Current day's usage
- **Week** - Current week's usage
- **Month** - Current month's usage
- **All** - All-time usage

### Refresh Button
Manually refresh usage data. Data also auto-refreshes every 10 seconds and when Claude Code writes new data.

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

### Token Summary
Four-column grid showing token counts for the selected period:
- **Input** - Standard input tokens
- **Output** - Output tokens generated
- **Cache R** - Cache read tokens (previously cached context)
- **Cache W** - Cache write tokens (new cached context)

### Models Section
Per-model quota usage (5hr rolling window):
- **Opus 4.5** (purple) - ~45 messages/5hr limit
- **Sonnet 4** (blue) - ~225 messages/5hr limit
- **Haiku 3.5** (green) - ~900 messages/5hr limit

Each model shows:
- Model name with message count (e.g., "12/45")
- Progress bar (green/yellow/red based on usage)
- Usage percentage

Sorted by highest usage percentage first.

### Active Sessions
Scrollable list of Claude Code sessions active in the last 24 hours. Each row displays:
- **Directory** - Project directory path (truncated from start, full path in tooltip)
- **Tokens** - Total token usage for the session
- **Duration** - How long the session has been active (e.g., "15m", "2h 30m")

Sorted by most recent activity.

### Last Updated
Timestamp showing when the data was last refreshed.

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
