# FiveM Firewall

Open **FXServer > Diagnostics > FiveM Firewall**, or use the Firewall button on Home.

1. Select the server profile, then choose **Read profile ports**. The app reads `endpoint_add_tcp` and `endpoint_add_udp` from `server.cfg` and its resolvable includes. It does not use the RCON, txAdmin, or database port.
2. Confirm the TCP and UDP ports. The initial values are FiveM's usual `30120`, not proof of your configuration. Multiple endpoints or unresolved includes need manual review.
3. Choose **Check firewall**. This is read-only and never requests elevation automatically.
4. If missing rules can be added, choose **Set up FiveM rules**, review the ports, and confirm. Approve the operating system's administrator prompt.
5. **Remove app rules** removes only this app's exact rules for the selected ports. Existing rules and default policies are left alone.

Both inbound and outbound traffic are checked. Outbound rules refer to the server's local/source game port. When the existing policy already allows a direction, no extra rule is added. A scoped rule or a conflicting block is not reported as universal access. Rules created for an old port remain until you explicitly inspect and remove that port's app rules.

## Supported Firewalls

- Windows Defender Firewall, including effective profile policies. Organization-managed restrictions may require your administrator.
- Linux UFW and firewalld in the separate Linux development branch. Use an already installed, active firewall. The app does not install, enable, disable, reset, or reload the entire firewall. Multiple active managers require manual review.
- Linux desktops require a functioning polkit authentication agent. On headless Linux, authorize `sudo -v` in the shell before reopening the terminal UI. The app uses noninteractive elevation and does not capture or store your sudo password.

Linux remains in development and is not included in the main Windows installer release. Other firewall managers, direct nftables/iptables policies, cloud security groups and routers must be configured separately.

## Status Is Not A Connectivity Test

An allowed host firewall rule does not prove that players can join. FXServer must be running and bound to a reachable address. Routers may need port forwarding, hosting providers may have a separate firewall, and carrier-grade NAT may prevent unsolicited inbound connections. Avoid exposing MariaDB or txAdmin publicly just to resolve a game connection problem.

Cancelled approval or a failed action is shown in the card and logged to Application Logs. Recheck after a timeout or partial failure before retrying. The app never removes external rules to make a check pass.

## Terminal Commands (Development Branch)

```text
fxins firewall check --tcp-port 30120 --udp-port 30120
fxins firewall check --backend ufw --elevated
fxins firewall setup --tcp-port 30120 --udp-port 30120 --yes
fxins firewall remove --tcp-port 30120 --udp-port 30120 --yes
```

`--yes` explicitly confirms changes; it is not an administrator-permission bypass. Run `fxins firewall --help` for subcommands. In the terminal UI, use the firewall actions to read profile ports, inspect, set up or remove rules without leaving the workspace.
