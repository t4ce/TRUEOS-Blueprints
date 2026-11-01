import unittest
from run import execute, location

class LabTests(unittest.TestCase):
    def run_js(self, source):
        return execute({'page': {'location': location('https://example.test/path')}, 'scripts': [{'url': 'fixture.js', 'source': source}]})
    def test_tree_identity_attributes_events_and_lazy_allocation(self):
        report = self.run_js('''
        const check = (v,m) => { if (!v) throw Error(m); };
        check(__domReport().allocatedNodes === 1, 'eager allocation');
        const el = document.getElementById('app');
        check(el === document.getElementById('app'), 'identity');
        check(el.isConnected && document.body.contains(el), 'connected');
        el.classList.add('one','two'); el.classList.remove('one');
        check(el.className === 'two', 'class');
        el.setAttribute('role','main'); check(el.getAttribute('role') === 'main','attribute');
        const text = document.createTextNode('hello'); el.appendChild(text);
        check(el.textContent === 'hello', 'text');
        const other = document.createElement('section'); document.body.appendChild(other); other.appendChild(text);
        check(text.parentNode === other && !el.hasChildNodes(), 'reparent');
        let clicks = 0; other.addEventListener('click', () => clicks++, {once:true}); other.click(); other.click();
        check(clicks === 1, 'event once');
        check(other.offsetWidth === 0 && other.getBoundingClientRect().width === 0,'layout');
        check(document.querySelectorAll('.absent').length === 0,'empty collection');
        const fragment = document.createDocumentFragment(); fragment.appendChild(el); other.appendChild(fragment);
        check(el.parentNode === other && !fragment.hasChildNodes(),'fragment');
        ''')
        self.assertEqual(report['errors'], [])
        self.assertEqual(report['unhandled_rejections'], [])
        self.assertTrue(report['host']['dom']['notes'])
    def test_storage_image_and_ready_events(self):
        report = self.run_js("""
        localStorage.setItem('answer',42);
        if (localStorage.getItem('answer') !== '42' || sessionStorage.getItem('answer') !== null) throw Error('storage');
        const image = new Image(20,10); image.src = '/image.png';
        if (image.tagName !== 'IMG' || image.width !== 20 || image.src !== '/image.png') throw Error('image');
        document.addEventListener('DOMContentLoaded', () => console.log('ready'));
        window.addEventListener('load', () => console.log('loaded'));
        """)
        self.assertEqual(report['errors'], [])
        self.assertEqual([x['message'] for x in report['host']['logs']], ['ready', 'loaded'])

    def test_timer_and_microtask(self):
        report = self.run_js("queueMicrotask(() => console.log('micro')); setTimeout(() => console.log('timer'), 5)")
        self.assertEqual([x['message'] for x in report['host']['logs']], ['micro', 'timer'])
    def test_errors_and_rejections_remain_visible(self):
        report = self.run_js("console.warn('visible'); Promise.reject('rejected'); throw Error('broken')")
        self.assertIn('broken', report['errors'][0]['error'])
        self.assertEqual(report['unhandled_rejections'], ['rejected'])
        self.assertEqual(report['host']['logs'][0]['level'], 'warn')
    def test_no_host_filesystem_or_network_api(self):
        report = self.run_js("if (typeof std !== 'undefined' || typeof os !== 'undefined' || typeof require !== 'undefined' || typeof fetch !== 'undefined') throw Error('host capability exposed')")
        self.assertEqual(report['errors'], [])
    def test_infinite_loop_interrupt(self):
        report = self.run_js('while (true) {}')
        self.assertIn('interrupted', report['errors'][0]['error'])

if __name__ == '__main__': unittest.main()
