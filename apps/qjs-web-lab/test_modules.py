import unittest
from run import execute, location, parse_imports, resolve_import, import_mapping


class ModuleTests(unittest.TestCase):
    def execute_graph(self, source, modules, resolutions=None, mapping=None):
        return execute({
            'page': {'location': location('https://fixture.test/')},
            'scripts': [{'url': 'https://fixture.test/main.js', 'source': source, 'module': True}],
            'modules': {url: {'source': body} for url, body in modules.items()},
            'module_resolutions': resolutions or {},
            'import_map': mapping or {},
        })

    def test_static_and_dynamic_imports_share_module_identity(self):
        source = """
        import {token} from './shared.js';
        import {again} from './consumer.js';
        if(token!==again)throw Error('module identity differs');
        import('./shared.js').then(module=>{
            if(module.token!==token||globalThis.sharedLoads!==1)throw Error('dynamic identity differs');
            console.log('module-identity-ok');
        });
        """
        report = self.execute_graph(source, {
            'https://fixture.test/shared.js': "globalThis.sharedLoads=(globalThis.sharedLoads||0)+1;export const token={};",
            'https://fixture.test/consumer.js': "import {token} from './shared.js';export {token as again};",
        }, {
            'https://fixture.test/main.js': {'./shared.js': 'https://fixture.test/shared.js', './consumer.js': 'https://fixture.test/consumer.js'},
            'https://fixture.test/consumer.js': {'./shared.js': 'https://fixture.test/shared.js'},
        })
        self.assertEqual(report['errors'], [])
        self.assertEqual(report['unhandled_rejections'], [])
        self.assertEqual([entry['message'] for entry in report['host']['logs']], ['module-identity-ok'])
        self.assertEqual(report['modules']['missing'], [])
        self.assertEqual(len(report['modules']['loaded']), 2)

    def test_import_map_bare_and_scoped_resolution(self):
        report = self.execute_graph("""
        import {value} from 'library';
        import {scopedValue} from 'https://fixture.test/scoped/consumer.js';
        if(value!==1||scopedValue!==2)throw Error('import-map resolution');
        """, {
            'https://fixture.test/root.js': 'export const value=1;',
            'https://fixture.test/scoped/library.js': 'export const value=2;',
            'https://fixture.test/scoped/consumer.js': "import {value} from 'library';export const scopedValue=value;",
        }, mapping={
            'imports': {'library': 'https://fixture.test/root.js'},
            'scopes': {'https://fixture.test/scoped/': {'library': 'https://fixture.test/scoped/library.js'}},
        })
        self.assertEqual(report['errors'], [])
        self.assertEqual(report['unhandled_rejections'], [])
        self.assertEqual(report['modules']['missing'], [])

    def test_entry_and_dependency_import_metadata(self):
        report=self.execute_graph("""
        import {url,main} from './dependency.js';
        if(import.meta.url!=='https://fixture.test/main.js'||import.meta.main!==true)throw Error('entry import.meta');
        if(url!=='https://fixture.test/dependency.js'||main!==false)throw Error('dependency import.meta');
        """,{'https://fixture.test/dependency.js':'export const url=import.meta.url;export const main=import.meta.main;'},
        {'https://fixture.test/main.js':{'./dependency.js':'https://fixture.test/dependency.js'}})
        self.assertEqual(report['errors'],[])
        self.assertEqual(report['unhandled_rejections'],[])
        self.assertEqual(report['modules']['missing'],[])

    def test_missing_dependency_stays_visible(self):
        report = self.execute_graph("import {value} from './missing.js';", {}, {
            'https://fixture.test/main.js': {'./missing.js': 'https://fixture.test/missing.js'},
        })
        self.assertTrue(report['errors'])
        self.assertIn('missing', report['errors'][0]['error'])
        self.assertTrue(report['modules']['missing'])
        self.assertEqual(report['modules']['loaded'], [])

    def test_module_cycle_uses_live_bindings(self):
        report = self.execute_graph("import {a} from './a.js';if(a()!==3)throw Error('cycle');", {
            'https://fixture.test/a.js': "import {b} from './b.js';export const a=()=>b()+1;export const seed=1;",
            'https://fixture.test/b.js': "import {seed} from './a.js';export const b=()=>seed+1;",
        }, {
            'https://fixture.test/main.js': {'./a.js': 'https://fixture.test/a.js'},
            'https://fixture.test/a.js': {'./b.js': 'https://fixture.test/b.js'},
            'https://fixture.test/b.js': {'./a.js': 'https://fixture.test/a.js'},
        })
        self.assertEqual(report['errors'], [])
        self.assertEqual(report['unhandled_rejections'], [])
        self.assertEqual(len(report['modules']['loaded']), 2)

    def test_parser_distinguishes_imports_from_comments_and_strings(self):
        parsed = parse_imports([{'url': 'https://fixture.test/main.js', 'module': True, 'source': """
        // import 'fake-comment';
        const text="import 'fake-string'";
        import {x} from './static.js';
        export {other} from './reexport.js';
        import('./literal.js');
        import(globalThis.computedModule);
        """}])
        self.assertEqual(parsed['parser'], 'acorn')
        result = parsed['results'][0]
        self.assertEqual(set(result['imports']), {'./static.js', './reexport.js', './literal.js'})
        self.assertEqual(result['computed_imports'], 1)
        self.assertNotIn('error', result)

    def test_controller_import_mapping_is_relative_to_page_base(self):
        issues = []
        mapping = import_mapping({'imports': {'library': './library.js', 'pkg/': './pkg/'}, 'scopes': {'./scoped/': {'library': './scoped/library.js'}}}, 'https://fixture.test/base/', issues)
        self.assertEqual(issues, [])
        self.assertEqual(resolve_import('library', 'https://fixture.test/base/main.js', mapping), 'https://fixture.test/base/library.js')
        self.assertEqual(resolve_import('library', 'https://fixture.test/base/scoped/main.js', mapping), 'https://fixture.test/base/scoped/library.js')
        self.assertEqual(resolve_import('pkg/item.js', 'https://fixture.test/base/main.js', mapping), 'https://fixture.test/base/pkg/item.js')
        self.assertEqual(resolve_import('../x.js', 'https://fixture.test/base/main.js', mapping), 'https://fixture.test/x.js')


if __name__ == '__main__':
    unittest.main()
