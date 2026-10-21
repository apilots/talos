"""Real TCP lifecycle checks for the private Chrome fixture's endpoint admission."""
import errno
import socket
import unittest

from native_engine import require_free_loopback_port


class EndpointAdmissionTests(unittest.TestCase):
    def test_rejects_an_active_listener_even_when_it_allows_address_reuse(self):
        with socket.socket() as listener:
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            listener.bind(('127.0.0.1', 0))
            listener.listen(1)
            with self.assertRaises(OSError) as rejected:
                require_free_loopback_port(listener.getsockname()[1])
            self.assertEqual(rejected.exception.errno, errno.EADDRINUSE)

    def test_accepts_the_closed_server_port_after_an_actual_tcp_connection(self):
        with socket.socket() as listener:
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            listener.bind(('127.0.0.1', 0))
            port = listener.getsockname()[1]
            listener.listen(1)
            with socket.create_connection(('127.0.0.1', port), timeout=2) as client:
                connection, _address = listener.accept()
                with connection:
                    # The server actively closes first and owns the TCP TIME_WAIT state.
                    connection.shutdown(socket.SHUT_WR)
                    self.assertEqual(client.recv(1), b'')
        require_free_loopback_port(port)


if __name__ == '__main__':
    unittest.main()
