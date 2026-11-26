"""Real model execution and callbacks through outbound Workers, reusing multi-node scenarios."""
from pathlib import Path
import subprocess
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'multi_node'))
from fixture import Cluster, require
from scenarios import routing, reliability, collaboration, parity
from workspace import WorkspaceCluster, workspace
from environment import EnvironmentCluster, environment
from configuration import configuration
from plugins import plugins
from oauth import OAuthCluster, oauth

cluster_type = {'workspace': WorkspaceCluster, 'environment': EnvironmentCluster, 'configuration': EnvironmentCluster, 'plugins': EnvironmentCluster, 'oauth': OAuthCluster}.get(sys.argv[1], Cluster)
cluster = cluster_type(worker_transport='outbound')
try:
    cluster.start()
    for node in cluster.nodes.values():
        listeners = subprocess.run(['lsof', '-nP', '-a', '-p', str(node['process'].pid), '-iTCP', '-sTCP:LISTEN'],
                                   capture_output=True, text=True)
        require(listeners.returncode == 1 and not listeners.stdout, 'Registered Worker opened a TCP listener')
    print('CASE outbound_workers_have_no_tcp_listener PASS', flush=True)
    {'routing': routing, 'reliability': reliability, 'collaboration': collaboration, 'parity': parity, 'workspace': workspace, 'environment': environment, 'configuration': configuration, 'plugins': plugins, 'oauth': oauth}[sys.argv[1]](cluster)
finally:
    cluster.close()
