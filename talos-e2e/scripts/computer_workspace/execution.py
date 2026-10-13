"""Real model execution and callbacks through outbound Workers, reusing legacy scenarios."""
from pathlib import Path
import subprocess
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'multi_node'))
from fixture import Cluster, require
from scenarios import routing, reliability, collaboration, parity
from workspace import WorkspaceCluster, workspace
from environment import EnvironmentCluster, environment
from configuration import ConfigurationCluster, configuration
from plugins import plugins
from oauth import OAuthCluster, oauth

cluster_type = {'workspace': WorkspaceCluster, 'environment': EnvironmentCluster, 'configuration': ConfigurationCluster, 'plugins': ConfigurationCluster, 'oauth': OAuthCluster}.get(sys.argv[1], Cluster)
cluster = cluster_type(worker_transport='outbound')
try:
    cluster.start()
    for node in cluster.nodes.values():
        listeners = subprocess.run(['lsof', '-nP', '-a', '-p', str(node['process'].pid), '-iTCP', '-sTCP:LISTEN'],
                                   capture_output=True, text=True)
        require(listeners.returncode == 0 and listeners.stdout, 'Outbound Worker RPC listener is missing')
        require(all('TCP 127.0.0.1:' in line for line in listeners.stdout.splitlines()[1:]),
                'Outbound Worker exposes a non-loopback listener')
    print('CASE outbound_workers_use_protected_loopback_rpc_listener PASS', flush=True)
    {'routing': routing, 'reliability': reliability, 'collaboration': collaboration, 'parity': parity, 'workspace': workspace, 'environment': environment, 'configuration': configuration, 'plugins': plugins, 'oauth': oauth}[sys.argv[1]](cluster)
finally:
    cluster.close()
