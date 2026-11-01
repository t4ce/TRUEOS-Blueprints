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
        check(Math.abs(other.offsetWidth - 3) < 1e-9 && Math.abs(other.getBoundingClientRect().width - 3) < 1e-9,'layout');
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

    def test_dotm_text_fonts_animation_and_host_state(self):
        report = self.run_js("""
        const check = (v,m) => {if(!v)throw Error(m);};
        const ctx=document.createElement('canvas').getContext('2d');
        ctx.font='1000px elaborate-family';
        const metrics=ctx.measureText('abc');
        check(Math.abs(metrics.width-1.8)<1e-9,'physical reference advance');
        check(Math.abs(metrics.actualBoundingBoxAscent+metrics.actualBoundingBoxDescent-1.1)<1e-9,'physical reference height');
        const node=document.createElement('span');node.textContent='abc';
        check(Math.abs(node.getBoundingClientRect().width-metrics.width)<1e-9,'DOM and canvas agree');
        check(Math.abs(node.getBoundingClientRect().height-1.1)<1e-9,'height');
        ctx.fillText('abc',0,0);check(__textReport().units==='DotM','units');
        check(node.animate([{opacity:0},{opacity:1}]).playState==='finished','animation');
        check(document.fonts.check('extravagant font'),'font availability');
        new FontFace('anything','url(ignored)').load().then(f=>check(f.status==='loaded','font load'));
        const a=performance.now(),b=performance.now();check(b>=a,'monotonic');
        const bytes=new Uint8Array(32);check(crypto.getRandomValues(bytes)===bytes&&bytes.some(x=>x!==0),'entropy');
        check(/^[0-9a-f-]{36}$/.test(crypto.randomUUID()),'UUID');
        document.cookie='test=value; Path=/';check(document.cookie==='test=value','cookie');
        const event=document.createEvent('Event');event.initEvent('test',true,true);event.preventDefault();check(event.defaultPrevented,'event');
        """)
        self.assertEqual(report['errors'], [])
        self.assertEqual(report['unhandled_rejections'], [])
        self.assertGreater(report['host']['api']['total'], 0)
        self.assertEqual(report['host']['text']['lineHeight']['unit'], 'DotM')
        self.assertAlmostEqual(report['host']['text']['lineHeight']['value'], 1.1)
        self.assertAlmostEqual(report['host']['text']['draws'][0]['width'], 1.8)

    def test_dotm_has_explicit_unit_boundaries_and_normalized_ratios(self):
        report = self.run_js("""
        const check=(v,m)=>{if(!v)throw Error(m);};
        const near=(a,b)=>Math.abs(a-b)<1e-9;
        const throws=fn=>{try{fn();return false;}catch{return true;}};
        check(DotM===DistanceOnTheMonitor,'alias');
        const reference=DotM.fromMillimeters(11);
        const advance=DotM.fromCentimeters(0.6);
        check(reference instanceof DotM && Object.isFrozen(reference),'distance type');
        check(near(reference.toCentimeters(),1.1)&&near(reference.toMillimeters(),11),'physical conversion');
        check(near(advance.ratio(reference),6/11),'distance ratio');
        check(near(DotM.fromNormalized(6/11,reference).toNumber(),0.6),'normalized requires reference');
        check(throws(()=>DotM.fromNormalized(1,1.1)),'untyped reference rejected');
        check(throws(()=>Number(reference))&&throws(()=>reference+1),'implicit unit loss rejected');
        check(throws(()=>reference.add(1))&&throws(()=>new DotM(Infinity)),'invalid distances rejected');
        check(near(reference.add(advance).subtract(reference).toNumber(),0.6),'typed arithmetic');
        const encoded=JSON.parse(JSON.stringify(reference));
        check(encoded.unit==='DotM'&&near(encoded.value,1.1),'typed transport');
        const m=MicrofontMetrics.measure('A\\nBC',reference);
        check(m.width instanceof DotM&&m.height instanceof DotM,'typed measurements');
        check(near(m.width.toNumber(),1.2)&&near(m.height.toNumber(),2.2),'multiline distance');
        check(MicrofontMetrics.normalized.lineHeight===1&&near(MicrofontMetrics.normalized.glyphAdvance,6/11),'dimensionless normalized geometry');
        const unicode=MicrofontMetrics.measure('A😀',reference.scale(2));
        check(near(unicode.width.toNumber(),2.4),'unicode scalar count');
        const ctx=document.createElement('canvas').getContext('2d');
        ctx.monitorEm=reference.scale(2);
        const node=document.createElement('span');node.monitorEm=ctx.monitorEm;node.textContent='abc';
        check(near(ctx.measureText('abc').width,3.6)&&near(node.getBoundingClientRect().width,3.6),'explicit em scaling');
        """)
        self.assertEqual(report['errors'], [])
        self.assertEqual(report['unhandled_rejections'], [])

    def test_shadow_fragments_events_and_custom_registry(self):
        report = self.run_js("""
        const check=(v,m)=>{if(!v)throw Error(m);};
        const host=document.createElement('div');document.body.appendChild(host);
        const root=host.attachShadow({mode:'open'});
        check(root instanceof ShadowRoot&&root instanceof DocumentFragment&&host.shadowRoot===root,'shadow identity');
        const fragment=document.createDocumentFragment(),child=document.createElement('span');
        fragment.appendChild(child);root.appendChild(fragment);
        check(child.parentNode===root&&!fragment.hasChildNodes()&&root.host===host,'shadow children');
        const mouse=document.createEvent('MouseEvent');
        mouse.initMouseEvent('click',true,true,null,0,0,0,1,2,false,false,false,false,0,null);
        check(mouse instanceof MouseEvent&&mouse.clientX===1,'mouse event type');
        mouse.preventDefault();check(mouse.defaultPrevented,'legacy event cancel');
        let received=0;child.addEventListener('detail',event=>received=event.detail.answer);
        child.dispatchEvent(new CustomEvent('detail',{detail:{answer:42}}));check(received===42,'custom event detail');
        const template=document.createElement('template');template.content.appendChild(document.createTextNode('saved'));
        const clone=template.cloneNode(true);check(clone.content.textContent==='saved'&&clone.content!==template.content,'template clone');
        class FixtureElement extends HTMLElement {}
        customElements.whenDefined('fixture-element').then(constructor=>check(constructor===FixtureElement,'when defined'));
        customElements.define('fixture-element',FixtureElement);
        check(customElements.get('fixture-element')===FixtureElement,'registry');
        customElements.upgrade(host);
        """)
        self.assertEqual(report['errors'], [])
        self.assertEqual(report['unhandled_rejections'], [])
        self.assertTrue(any(n['kind']=='custom-element-upgrade-skipped' for n in report['host']['dom']['notes']))

    def test_trace_counts_and_bounded_failure_history(self):
        report = self.run_js("""
        for(let i=0;i<240;i++)document.createElement('div');
        throw Error('after-host-calls');
        """)
        self.assertIn('after-host-calls', report['errors'][0]['error'])
        snapshot=report['errors'][0]['api_trace']
        self.assertGreaterEqual(snapshot['counts']['Document.createElement'],240)
        self.assertEqual(len(snapshot['recent']),200)
        self.assertEqual(snapshot['recent'][-1]['phase'],'fixture.js')
        self.assertEqual(snapshot['recent'][-1]['api'],'Document.createElement')
        self.assertEqual(snapshot['recent'][-1]['args'],['div'])
        self.assertGreaterEqual(report['host']['api']['total'],snapshot['total'])

    def test_listener_error_is_reported_and_remaining_listeners_run(self):
        report=self.run_js("""
        const button=document.createElement('button');
        button.addEventListener('click',()=>{throw Error('listener-broken');});
        button.addEventListener('click',()=>console.log('listener-after-error'));
        button.click();
        """)
        self.assertEqual(report['errors'],[])
        self.assertEqual([x['message'] for x in report['host']['logs']],['listener-after-error'])
        self.assertEqual(len(report['host']['errors']),1)
        self.assertIn('listener-broken',report['host']['errors'][0]['error'])
        self.assertEqual(report['host']['errors'][0]['phase'],'event:click')
        self.assertGreater(report['host']['errors'][0]['api_trace']['total'],0)

    def test_microtasks_run_between_timer_tasks_and_intervals_cancel(self):
        report=self.run_js("""
        setTimeout(()=>{console.log('first');queueMicrotask(()=>console.log('micro'));},0);
        setTimeout(()=>console.log('second'),0);
        let ticks=0;const id=setInterval(()=>{if(++ticks===2){clearInterval(id);console.log('interval-done');}},1);
        """)
        self.assertEqual(report['errors'],[])
        self.assertEqual([x['message'] for x in report['host']['logs']],['first','micro','second','interval-done'])
        self.assertEqual(report['host']['pendingTimers'],0)

    def test_message_channel_clones_data_and_schedules_tasks(self):
        report = self.run_js("""
        const channel = new MessageChannel(), input={value:1};input.self=input;
        channel.port2.onmessage=event=>{if(event.data.value!==1||event.data.self!==event.data)throw Error('clone');console.log('message');channel.port2.close();};
        channel.port1.postMessage(input);input.value=2;
        Promise.resolve().then(()=>console.log('microtask'));
        console.log('sync');
        const observer = new IntersectionObserver(()=>{throw Error('inactive callback');});observer.observe(document.body);observer.disconnect();
        """)
        self.assertEqual(report['errors'], [])
        self.assertEqual(report['host']['errors'], [])
        self.assertEqual([entry['message'] for entry in report['host']['logs']], ['sync','microtask','message'])
        self.assertTrue(any(note['kind']=='observer-inactive' for note in report['host']['text']['notes']))

    def test_html_type_identity_and_memory_history(self):
        report = self.run_js("""
        const div=document.createElement('div'),span=document.createElement('span');
        if(!(div instanceof HTMLDivElement)||span instanceof HTMLDivElement)throw Error('tag identity');
        if(!(document.createElement('video') instanceof HTMLMediaElement))throw Error('media identity');
        if(!(window instanceof Window))throw Error('window identity');
        const state={value:1};history.pushState(state,'','?example=1');state.value=2;
        if(history.state.value!==1||history.length!==2)throw Error('history cloning');
        window.addEventListener('popstate',event=>{if(event.state!==null)throw Error('initial history');console.log('back');});history.back();
        """)
        self.assertEqual(report['errors'], [])
        self.assertEqual(report['host']['errors'], [])
        self.assertEqual([entry['message'] for entry in report['host']['logs']], ['back'])

    def test_timer_and_microtask(self):
        report = self.run_js("queueMicrotask(() => console.log('micro')); setTimeout(() => console.log('timer'), 5)")
        self.assertEqual([x['message'] for x in report['host']['logs']], ['micro', 'timer'])
    def test_errors_and_rejections_remain_visible(self):
        report = self.run_js("console.warn('visible'); Promise.reject('rejected'); throw Error('broken')")
        self.assertIn('broken', report['errors'][0]['error'])
        self.assertEqual(report['unhandled_rejections'], ['rejected'])
        self.assertEqual(report['host']['logs'][0]['level'], 'warn')
    def test_script_completion_object_is_not_coerced(self):
        report=execute({
            'page':{'location':location('https://example.test/')},
            'scripts':[
                {'url':'completion.js','source':"globalThis.completionCoercions=0;({toString(){completionCoercions++;throw Error('completion-was-coerced');}})"},
                {'url':'verify.js','source':"if(completionCoercions!==0)throw Error('completion-side-effect');console.log('completion-discarded');"},
            ],
        })
        self.assertEqual(report['errors'],[])
        self.assertEqual(report['unhandled_rejections'],[])
        self.assertEqual([entry['message'] for entry in report['host']['logs']],['completion-discarded'])
        self.assertEqual(report['scripts_attempted'],2)

    def test_no_host_filesystem_or_network_api(self):
        report = self.run_js("if (typeof std !== 'undefined' || typeof os !== 'undefined' || typeof require !== 'undefined' || typeof fetch !== 'undefined') throw Error('host capability exposed')")
        self.assertEqual(report['errors'], [])
    def test_infinite_loop_interrupt(self):
        report = self.run_js('while (true) {}')
        self.assertIn('interrupted', report['errors'][0]['error'])

if __name__ == '__main__': unittest.main()
