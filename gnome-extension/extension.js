/**
 * CC Usage Widget GNOME Extension
 *
 * Displays Claude Code weekly usage as a progress bar in the GNOME Shell panel.
 * Connects to the CC Usage Widget Tauri app via D-Bus.
 */

import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import GObject from 'gi://GObject';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';

const DBUS_NAME = 'com.shane.CCUsageWidget';
const DBUS_PATH = '/com/shane/CCUsageWidget';
const DBUS_INTERFACE = 'com.shane.CCUsageWidget1';

// Progress bar dimensions
const BAR_WIDTH = 60;
const BAR_HEIGHT = 14;
const BLOCK_COUNT = 8;

const CCUsageIndicator = GObject.registerClass(
    class CCUsageIndicator extends PanelMenu.Button {
        _init() {
            super._init(0.0, 'CC Usage Widget');

            this._usagePercent = 0;
            this._daysLeft = 0;

            // Container for the progress bar
            this._box = new St.BoxLayout({
                style_class: 'cc-usage-box',
            });
            this.add_child(this._box);

            // Drawing area for the progress bar
            this._drawingArea = new St.DrawingArea({
                width: BAR_WIDTH,
                height: BAR_HEIGHT,
                style_class: 'cc-usage-bar',
            });
            this._drawingArea.connect('repaint', this._onRepaint.bind(this));
            this._box.add_child(this._drawingArea);

            // Tooltip label (shown on hover)
            this._tooltipLabel = new St.Label({
                style_class: 'cc-usage-tooltip',
                text: '',
                visible: false,
            });
            Main.uiGroup.add_child(this._tooltipLabel);

            // Connect hover events
            this.connect('enter-event', this._onEnter.bind(this));
            this.connect('leave-event', this._onLeave.bind(this));

            // D-Bus proxy
            this._proxy = null;
            this._updateTimeout = null;

            this._createProxy();
        }

        _createProxy() {
            const proxyInfo = Gio.DBusInterfaceInfo.new_for_xml(`
                <node>
                    <interface name="${DBUS_INTERFACE}">
                        <method name="GetUsageSummary">
                            <arg type="d" direction="out" name="week_usage_percent"/>
                            <arg type="u" direction="out" name="days_left"/>
                        </method>
                    </interface>
                </node>
            `);

            Gio.DBusProxy.new(
                Gio.bus_get_sync(Gio.BusType.SESSION, null),
                Gio.DBusProxyFlags.NONE,
                proxyInfo,
                DBUS_NAME,
                DBUS_PATH,
                DBUS_INTERFACE,
                null,
                this._onProxyReady.bind(this)
            );
        }

        _onProxyReady(source, result) {
            try {
                this._proxy = Gio.DBusProxy.new_finish(result);
                this._fetchUsage();
                // Poll every 30 seconds for updates
                this._updateTimeout = GLib.timeout_add_seconds(
                    GLib.PRIORITY_DEFAULT,
                    30,
                    () => {
                        this._fetchUsage();
                        return GLib.SOURCE_CONTINUE;
                    }
                );
            } catch (e) {
                console.error(`CC Usage Widget: Failed to create D-Bus proxy: ${e.message}`);
            }
        }

        _fetchUsage() {
            if (!this._proxy) return;

            this._proxy.call(
                'GetUsageSummary',
                null,
                Gio.DBusCallFlags.NONE,
                -1,
                null,
                (proxy, result) => {
                    try {
                        const reply = proxy.call_finish(result);
                        const [usagePercent, daysLeft] = reply.deep_unpack();
                        this._usagePercent = usagePercent;
                        this._daysLeft = daysLeft;
                        this._drawingArea.queue_repaint();
                        this._updateTooltip();
                    } catch (e) {
                        // D-Bus call failed, app might not be running
                        console.debug(`CC Usage Widget: D-Bus call failed: ${e.message}`);
                    }
                }
            );
        }

        _onRepaint(area) {
            const cr = area.get_context();
            const [width, height] = area.get_surface_size();

            // Background
            cr.setSourceRGBA(0.2, 0.2, 0.2, 0.8);
            cr.rectangle(0, 0, width, height);
            cr.fill();

            // Determine color based on usage
            const percent = this._usagePercent;
            let r, g, b;
            if (percent < 60) {
                // Green
                r = 0.3;
                g = 0.8;
                b = 0.3;
            } else if (percent < 85) {
                // Yellow
                r = 0.9;
                g = 0.8;
                b = 0.2;
            } else {
                // Red
                r = 0.9;
                g = 0.3;
                b = 0.3;
            }

            // Draw progress blocks
            const blockWidth = (width - 4) / BLOCK_COUNT - 2;
            const blockHeight = height - 4;
            const filledBlocks = Math.round((percent / 100) * BLOCK_COUNT);

            for (let i = 0; i < BLOCK_COUNT; i++) {
                const x = 2 + i * (blockWidth + 2);
                const y = 2;

                if (i < filledBlocks) {
                    // Filled block
                    cr.setSourceRGBA(r, g, b, 1.0);
                } else {
                    // Empty block
                    cr.setSourceRGBA(0.4, 0.4, 0.4, 0.5);
                }

                cr.rectangle(x, y, blockWidth, blockHeight);
                cr.fill();
            }

            cr.$dispose();
        }

        _updateTooltip() {
            const percent = Math.round(this._usagePercent);
            const days = this._daysLeft;
            this._tooltipLabel.text = `${percent}% | ${days}d Left`;
        }

        _onEnter() {
            this._updateTooltip();
            this._tooltipLabel.visible = true;

            // Position tooltip below the indicator
            const [x, y] = this.get_transformed_position();
            const [width, height] = this.get_size();
            this._tooltipLabel.set_position(
                x + width / 2 - this._tooltipLabel.width / 2,
                y + height + 5
            );

            return Clutter.EVENT_PROPAGATE;
        }

        _onLeave() {
            this._tooltipLabel.visible = false;
            return Clutter.EVENT_PROPAGATE;
        }

        destroy() {
            if (this._updateTimeout) {
                GLib.source_remove(this._updateTimeout);
                this._updateTimeout = null;
            }

            if (this._tooltipLabel) {
                Main.uiGroup.remove_child(this._tooltipLabel);
                this._tooltipLabel.destroy();
                this._tooltipLabel = null;
            }

            this._proxy = null;
            super.destroy();
        }
    }
);

export default class CCUsageWidgetExtension {
    constructor() {
        this._indicator = null;
        this._watcherId = null;
    }

    enable() {
        // Watch for the D-Bus name to appear/disappear
        this._watcherId = Gio.bus_watch_name(
            Gio.BusType.SESSION,
            DBUS_NAME,
            Gio.BusNameWatcherFlags.NONE,
            this._onNameAppeared.bind(this),
            this._onNameVanished.bind(this)
        );
    }

    disable() {
        if (this._watcherId) {
            Gio.bus_unwatch_name(this._watcherId);
            this._watcherId = null;
        }

        this._removeIndicator();
    }

    _onNameAppeared() {
        console.log('CC Usage Widget: D-Bus service appeared');
        if (!this._indicator) {
            this._indicator = new CCUsageIndicator();
            Main.panel.addToStatusArea('cc-usage-widget', this._indicator);
        }
    }

    _onNameVanished() {
        console.log('CC Usage Widget: D-Bus service vanished');
        this._removeIndicator();
    }

    _removeIndicator() {
        if (this._indicator) {
            this._indicator.destroy();
            this._indicator = null;
        }
    }
}
