"""Fixed required contexts validate selected jobs, including failure/cancellation."""
import json
import os
import sys


def validate(mode, plan, needs):
    if needs['scope']['result'] != 'success':
        raise ValueError('Module change detection did not succeed')
    categories = plan['categories']
    expected = {c: bool(task['quality'] or task['scenarios']['include']) for c, task in categories.items()}
    expected.update(scope=True, build=bool(plan['build']['binaries']), tests=bool(plan['test_build']['tests']),
                    installer=plan['installer'], harness=plan['harness'])
    owners = {'frontend': ('scope', 'webui', 'desktop'), 'rust': ('scope', 'server', 'tui'),
              'installer': ('scope', 'installer'),
              'e2e': ('scope', 'build', 'tests', 'webui', 'desktop', 'server', 'tui', 'harness')}[mode]
    for owner in owners:
        wanted = 'success' if expected[owner] else 'skipped'
        if needs[owner]['result'] != wanted:
            raise ValueError(f'{owner}: expected {wanted}, received {needs[owner]["result"]}')


if __name__ == '__main__':
    validate(sys.argv[1], json.loads(os.environ['CI_PLAN']), json.loads(os.environ['CI_NEEDS']))
    print('All selected module checks passed; unselected modules skipped.')
