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
        checks = [call.args for call in run.call_args_list[:4]]
        base = ('just', 'check', '--locked', '-p', 'talos-base', '--no-default-features')
        self.assertEqual(checks, [base, base + ('--features', 'sync'), base + ('--features', 'relay'), base + ('--features', 'sync,relay')])
        capture = run.call_args_list[4]
        self.assertEqual(capture.args, ('just', 'test', '--locked', '-p', 'talos-store', '--no-default-features'))
        self.assertNotIn('TALOS_TEST_ARCHIVE', capture.kwargs['env'])


if __name__ == '__main__':
    unittest.main()
