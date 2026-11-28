"""CI composition gate selection, including archive isolation for capture tests."""
import os
import unittest
from unittest.mock import patch
import ci_run


class ProductFeatureGateTests(unittest.TestCase):
    def test_leaf_packages_do_not_expand_the_runtime_matrix(self):
        with patch.object(ci_run, 'run') as run:
            ci_run.check_product_features(['talos-auth'])
            run.assert_not_called()

    def test_runtime_checks_all_combinations_and_real_capture_only_tests(self):
        with patch.dict(os.environ, {'TALOS_TEST_ARCHIVE': '/archive/default.tar.zst'}):
            with patch.object(ci_run, 'run') as run:
                ci_run.check_product_features(['talos-base'])
        self.assertEqual(run.call_args_list[0].args,
                         ('just', 'check', '--locked', '-p', 'talos-worker-bin', '--bin', 'talos-worker'))
        checks = [call.args for call in run.call_args_list[1:9]]
        base = ('just', 'check', '--locked', '-p', 'talos-base', '--no-default-features', '--all-targets')
        self.assertEqual(checks, [
            base, base + ('--features', 'sync'), base + ('--features', 'relay-client'),
            base + ('--features', 'sync,relay-client'), base + ('--features', 'server'),
            base + ('--features', 'server,sync'), base + ('--features', 'server,relay'),
            base + ('--features', 'server,sync,relay'),
        ])
        suites = run.call_args_list[9:]
        self.assertEqual([call.args for call in suites], [
            ('just', 'test', '--locked', '-p', 'talos-store', '--no-default-features'),
            ('just', 'test', '--locked', '-p', 'talos-base', '--no-default-features'),
            ('just', 'test', '--locked', '-p', 'talos-base', '--no-default-features',
             '--features', 'server'),
            ('just', 'test', '--locked', '-p', 'talos-base', '--no-default-features',
             '--features', 'server,sync,relay,relay-client'),
        ])
        for suite in suites:
            self.assertNotIn('TALOS_TEST_ARCHIVE', suite.kwargs['env'])



if __name__ == '__main__':
    unittest.main()
