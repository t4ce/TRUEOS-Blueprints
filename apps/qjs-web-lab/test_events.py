import unittest
from run import execute, location


class EventScenarioTests(unittest.TestCase):
    def run_events(self, source, events=None):
        payload = {
            'page': {'location': location('https://fixture.test/')},
            'scripts': [{'url': 'events.js', 'source': source}],
        }
        if events is not None:
            payload['events'] = events
        return execute(payload)

    def test_registered_visibility_and_resize_handlers_change_state(self):
        report = self.run_events("""
        document.addEventListener('visibilitychange', () => {
            if(document.hidden)return;
            const node=document.createElement('span');node.textContent='visible';document.body.appendChild(node);
        });
        window.addEventListener('resize', () => {
            const node=document.createElement('div');node.textContent='resized';document.body.appendChild(node);
        });
        """, [
            {'name': 'hidden', 'type': 'visibilitychange', 'target': 'document', 'state': {'visibilityState': 'hidden'}},
            {'name': 'visible', 'type': 'visibilitychange', 'target': 'document', 'state': {'visibilityState': 'visible'}},
            {'name': 'resized', 'type': 'resize', 'target': 'window', 'state': {'innerWidth': 40, 'innerHeight': 20}},
        ])
        self.assertEqual(report['errors'], [])
        baseline, hidden, visible, resized = report['checkpoints']
        self.assertEqual(baseline['allocated_nodes'], 1)
        self.assertEqual(baseline['text_characters'], 0)
        self.assertFalse(hidden['state_progress'])
        self.assertTrue(visible['state_progress'])
        self.assertEqual(visible['delta']['text_characters'], 7)
        self.assertTrue(resized['state_progress'])
        self.assertEqual(resized['delta']['text_characters'], 7)
        self.assertEqual(resized['text_characters'], 14)
        self.assertTrue(all(c.get('dispatched', True) for c in report['checkpoints']))

    def test_unhandled_event_is_not_claimed_as_state_progress(self):
        report = self.run_events('', [{'type': 'pageshow', 'target': 'window'}])
        baseline, event = report['checkpoints']
        self.assertEqual(baseline['api_calls'], event['api_calls'] - event['delta']['api_calls'])
        self.assertGreaterEqual(event['delta']['api_calls'], 1)
        self.assertEqual(event['delta']['allocated_nodes'], 0)
        self.assertEqual(event['delta']['connected_nodes'], 0)
        self.assertEqual(event['delta']['text_characters'], 0)
        self.assertFalse(event['state_progress'])
        self.assertTrue(event['dispatched'])
        self.assertFalse(event['is_trusted'])
        self.assertEqual(report['host']['errors'], [])

    def test_event_errors_stay_visible_while_other_listeners_continue(self):
        report = self.run_events("""
        window.addEventListener('resize', () => {throw Error('resize-listener-broken');});
        window.addEventListener('resize', () => {
            const node=document.createElement('span');node.textContent='after error';document.body.appendChild(node);
        });
        """, [{'type': 'resize', 'target': 'window'}])
        event = report['checkpoints'][1]
        self.assertEqual(report['errors'], [])
        self.assertEqual(event['delta']['host_errors'], 1)
        self.assertEqual(event['host_errors'], 1)
        self.assertTrue(event['state_progress'])
        self.assertIn('resize-listener-broken', report['host']['errors'][0]['error'])

    def test_event_microtasks_and_timers_are_included_in_checkpoint(self):
        report = self.run_events("""
        window.addEventListener('pageshow', () => {
            Promise.resolve().then(() => {
                const node=document.createElement('span');node.textContent='micro';document.body.appendChild(node);
            });
            setTimeout(() => {
                const node=document.createElement('span');node.textContent='timer';document.body.appendChild(node);
            }, 0);
        });
        """, [{'type': 'pageshow', 'target': 'window'}])
        event = report['checkpoints'][1]
        self.assertEqual(report['errors'], [])
        self.assertEqual(event['delta']['text_characters'], 10)
        self.assertEqual(event['pending_timers'], 0)
        self.assertFalse(event['pending_jobs'])
        self.assertTrue(event['state_progress'])

    def test_event_promise_rejection_is_counted_at_checkpoint(self):
        report = self.run_events("window.addEventListener('pageshow',()=>Promise.reject('event-rejected'));", [{'type': 'pageshow'}])
        event = report['checkpoints'][1]
        self.assertEqual(event['delta']['unhandled_rejections'], 1)
        self.assertEqual(report['unhandled_rejections'], ['event-rejected'])
        self.assertFalse(event['state_progress'])

    def test_selector_event_targets_existing_node_and_missing_target_is_not_created(self):
        report = self.run_events("""
        const node=document.createElement('button');node.id='actual';document.body.appendChild(node);
        node.addEventListener('show',event=>{if(event.isTrusted!==false)throw Error('trusted');node.textContent=event.detail;});
        """, [
            {'type': 'show', 'target': {'selector': '#actual'}, 'init': {'detail': 'existing'}},
            {'type': 'show', 'target': '#missing'},
        ])
        baseline, existing, missing = report['checkpoints']
        self.assertTrue(existing['dispatched'])
        self.assertEqual(existing['delta']['text_characters'], 8)
        self.assertFalse(missing['dispatched'])
        self.assertFalse(missing['state_progress'])
        self.assertEqual(missing['delta']['allocated_nodes'], 0)
        self.assertEqual(missing['delta']['errors'], 1)
        self.assertIn('does not exist', report['errors'][0]['error'])

    def test_sampling_without_scenarios_does_not_create_nodes_or_call_dom_apis(self):
        report = self.run_events('')
        self.assertEqual(len(report['checkpoints']), 1)
        baseline = report['checkpoints'][0]
        self.assertEqual(baseline['kind'], 'baseline')
        self.assertEqual(baseline['allocated_nodes'], 1)
        self.assertEqual(baseline['connected_nodes'], 1)
        self.assertEqual(baseline['api_calls'], report['host']['api']['total'])
        self.assertEqual(baseline['text_characters'], 0)
        self.assertEqual(baseline['text_draw_calls'], 0)
        self.assertFalse(baseline['state_progress'])


    def test_chrome_like_lifecycle_has_task_boundaries_and_single_window_bubble(self):
        source="""
        document.addEventListener('readystatechange',()=>console.log('state:'+document.readyState));
        document.addEventListener('DOMContentLoaded',()=>{
            console.log('document:'+document.readyState);
            Promise.resolve().then(()=>console.log('micro:'+document.readyState));
        });
        window.addEventListener('DOMContentLoaded',()=>console.log('window:'+document.readyState));
        window.addEventListener('load',()=>console.log('load:'+document.readyState));
        window.addEventListener('pageshow',event=>console.log('pageshow:'+document.readyState+':'+event.persisted));
        """
        report=execute({'page':{'location':location('https://fixture.test/')},'scripts':[{'url':'lifecycle.js','source':source}],'lifecycle_mode':'chrome'})
        self.assertEqual(report['errors'],[])
        self.assertEqual(report['host']['errors'],[])
        self.assertEqual(report['lifecycle_mode'],'chrome')
        self.assertEqual([entry['message'] for entry in report['host']['logs']],
            ['state:interactive','document:interactive','window:interactive','micro:interactive','state:complete','load:complete','pageshow:complete:false'])

    def test_default_lifecycle_remains_legacy(self):
        report=self.run_events("""
        document.addEventListener('readystatechange',()=>console.log('state'));
        document.addEventListener('DOMContentLoaded',()=>{
            console.log('document:'+document.readyState);
            Promise.resolve().then(()=>console.log('micro:'+document.readyState));
        });
        window.addEventListener('DOMContentLoaded',()=>console.log('window'));
        window.addEventListener('load',()=>console.log('load:'+document.readyState));
        window.addEventListener('pageshow',()=>console.log('pageshow'));
        """)
        self.assertEqual(report['errors'],[])
        self.assertEqual(report['lifecycle_mode'],'legacy')
        self.assertEqual([entry['message'] for entry in report['host']['logs']],['document:interactive','load:complete','micro:complete'])

    def test_unknown_lifecycle_mode_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError,'lifecycle_mode must be legacy or chrome'):
            execute({'page':{'location':location('https://fixture.test/')},'scripts':[],'lifecycle_mode':'silently-guess'})


if __name__ == '__main__':
    unittest.main()
