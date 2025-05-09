# Wicked to NetworkManager migration
This project creates a `wicked2nm` binary which is able to parse wicked xml
configs and send them to a NetworkManager dbus service.
## Installation
`wicked2nm` is available in openSUSE tumbleweed `zypper in wicked2nm`.

There is also a package that includes all the latest changes available at
https://build.opensuse.org/package/show/home:jcronenberg:migrate-wicked/wicked2nm.
## Usage
### On system
If both `wicked` and `NetworkManager` are available `wicked2nm` can be run on the system directly simply with:
```bash
wicked show-config > wicked.xml
wicked2nm migrate wicked.xml
```
More detailed instructions how a migration can work here:
```bash
# NetworkManager-config-server is required as otherwise NM will immediately add connections for all interfaces, resulting in duplicates.
# NetworkManager-config-server can be removed after the migration is done.
zypper in wicked2nm NetworkManager NetworkManager-config-server
# If NetworkManager-config-server is not available you can also manually add the drop-in configuration.
echo -e "[main]\nno-auto-default=*" > /etc/NetworkManager/conf.d/10-server.conf

# You can test beforehand whether there are errors or warnings.
wicked show-config | wicked2nm migrate --dry-run -

# WARNING: Run this as root, wicked will shut down the interfaces and they will only come up again once the migration is done.
# This oneliner shuts down wicked, starts NM and runs the migration, if anything went wrong it starts wicked again.
systemctl disable --now wicked \
    && (systemctl enable --now NetworkManager && wicked show-config | wicked2nm migrate --continue-migration --activate-connections -) \
    || (systemctl disable --now NetworkManager; systemctl enable --now wicked)
```
### Via container
`wicked2nm` can also be run via a container.
```bash
podman run -v /etc/sysconfig/network:/etc/sysconfig/network registry.opensuse.org/home/jcronenberg/migrate-wicked/containers/opensuse/wicked2nm:latest
```
This will create `*.nmconnection` files inside `/etc/sysconfig/network/NM-migrated`.  
See also the [Container's README](https://build.opensuse.org/projects/home:jcronenberg:migrate-wicked/packages/wicked2nm-container/files/README?expand=1)
for further infos.
## Architecture
`wicked2nm` uses agama as a library to communicate the parsed network state to NetworkManager
but the binary is completely independent of any agama services and can be run standalone.
