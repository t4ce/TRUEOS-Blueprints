#include "scene.hpp"
#include <iostream>
#include <random>
#include <functional>
using namespace rfhost;
static uint tests=0;
static void check(bool b,const char* m){if(!b)throw std::runtime_error(m);}
static bool close(float a,float b,float eps=1e-4f){return std::fabs(a-b)<=eps*std::max(1.0f,std::max(std::fabs(a),std::fabs(b)));}
static bool close(V a,V b,float e=1e-4f){return close(a.x,b.x,e)&&close(a.y,b.y,e)&&close(a.z,b.z,e);}
static void test(const char* name,const std::function<void()>&fn){fn();++tests;std::cout<<"PASS "<<name<<'\n';}
static Scene basic(){Scene s;s.surfaces.push_back({v(0,0,0),v(0,0,1)});return s;}
static void no_environment(std::vector<uint>&w){put(w,20,v(0,0,0));put(w,24,v(0,0,0));put(w,19,0);}
static void roof(Scene&s,uint mat=0){s.triangles.push_back({v(-10,-10,1),v(10,-10,1),v(10,10,1),mat});s.triangles.push_back({v(-10,-10,1),v(10,10,1),v(-10,10,1),mat});}
static std::vector<uint> lists(const std::vector<uint>&w){std::vector<uint>x(((w[3]+63)/64)*rf::LIST_WORDS);for(uint i=0;i<(w[3]+63)/64;++i)rf::build_light_list(w.data(),x.data(),i);return x;}
static rf::Hit brute(const std::vector<uint>&w,V o,V d,float max_t){rf::Hit hit={max_t,rf::INVALID};for(uint i=0;i<w[5];++i){float t=rf::triangle_hit(w.data(),i,o,d,.0001f,hit.t);if(t<hit.t)hit={t,i};}return hit;}
static Scene fixture(){
    Scene s;s.materials={{{.5f,.5f,.5f},{0,0,0}},{{.8f,.06f,.025f},{.3f,.02f,0}}};
    // A wall with a partial roof: deterministic, synthetic inputs, NOT a Warcraft capture.
    for(int k=0;k<8;++k){float x=float(k)-4;s.triangles.push_back({v(x,2,0),v(x+1,2,0),v(x+1,2,3),1});s.triangles.push_back({v(x,2,0),v(x+1,2,3),v(x,2,3),1});}
    s.triangles.push_back({v(-2,-1,1.2f),v(0,-1,1.2f),v(0,1,1.2f),0});s.triangles.push_back({v(-2,-1,1.2f),v(0,1,1.2f),v(-2,1,1.2f),0});
    // Deliberately >8 lights; many overlap to exercise correct overflow.
    for(uint i=0;i<65;++i){float angle=2*rf::PI*float(i)/65; s.lights.push_back({v(3*std::cos(angle),3*std::sin(angle),2),v(.04f,.02f,.01f),8,.1f});}
    for(uint y=0;y<8;++y)for(uint x=0;x<16;++x)s.surfaces.push_back({v((float(x)-7.5f)*.5f,(float(y)-3.5f)*.5f,0),v(0,0,1)});
    s.surfaces.push_back({v(1,0,.1f),v(0,0,1),v(.12f,.2f,.3f),0,rf::SURFACE_VALID|rf::SURFACE_WATER});
    return s;
}
int main(int argc,char**argv){try{
    test("empty scene validates",[]{auto s=basic();auto w=s.pack();validate(w);check(rf::trace(w.data(),v(0,0,0),v(0,0,1),.001f,100,false).tri==rf::INVALID,"empty hit");});
    test("parallel AABB rays",[]{check(rf::box_hit(v(0,0,-2),v(0,0,1),v(-1,-1,-1),v(1,1,1),100),"parallel inside");check(!rf::box_hit(v(2,0,-2),v(0,0,1),v(-1,-1,-1),v(1,1,1),100),"parallel outside");});
    test("triangle two sided and finite segment",[]{auto s=basic();roof(s);auto w=s.pack();validate(w);check(close(rf::trace(w.data(),v(0,0,0),v(0,0,1),.001f,100,false).t,1),"front hit");check(close(rf::trace(w.data(),v(0,0,2),v(0,0,-1),.001f,100,false).t,1),"back hit");check(rf::trace(w.data(),v(0,0,0),v(0,0,1),.001f,.5f,true).tri==rf::INVALID,"range hit");});
    test("threaded BVH matches brute force: 5000 random rays",[]{Scene s;std::mt19937 r(12345);std::uniform_real_distribution<float>d(-3,3);for(int i=0;i<96;++i){V a=v(d(r),d(r),d(r));s.triangles.push_back({a,add(a,v(.3f,0,.2f)),add(a,v(0,.7f,0)),0});}auto w=s.pack();validate(w);for(int i=0;i<5000;++i){V o=v(d(r),d(r),d(r)),dir=rf::norm(v(d(r),d(r),d(r)));auto a=rf::trace(w.data(),o,dir,.0001f,100,false),b=brute(w,o,dir,100);check((a.tri==rf::INVALID)==(b.tri==rf::INVALID)&&close(a.t,b.t),"BVH disagrees");}});
    test("no light means zero direct",[]{auto s=basic();auto w=s.pack();no_environment(w);check(close(rf::direct(w.data(),0),v(0,0,0)),"unlit direct");});
    test("sun shadow changes actual visibility",[]{auto s=basic();auto w=s.pack();put(w,16,v(0,0,1));put(w,19,0);V open=rf::direct(w.data(),0);roof(s);auto closed=s.pack();put(closed,16,v(0,0,1));put(closed,19,0);check(open.x>0&&close(rf::direct(closed.data(),0),v(0,0,0)),"shadow missing");});
    test("65 local lights contribute, independently of GL",[]{auto s=basic();s.lights.push_back({v(0,0,2),v(1,.5f,.25f),10,.1f});auto one=s.pack();no_environment(one);V a=rf::direct(one.data(),0);for(int i=1;i<65;++i)s.lights.push_back(s.lights[0]);auto many=s.pack();no_environment(many);check(close(rf::direct(many.data(),0),mul(a,65)),"eight-light cap");});
    test("cluster list exactly matches direct reference",[]{auto s=basic();s.lights={{{0,0,2},{1,1,1},10,.1f},{{90,0,2},{1,1,1},2,.1f}};auto w=s.pack();auto l=lists(w);check(l[0]==1&&l[1]==0,"list cull");check(close(rf::direct_listed(w.data(),l.data(),0),rf::direct(w.data(),0)),"list result");});
    test("overflow does not drop lights",[]{auto s=fixture();auto w=s.pack();validate(w);auto l=lists(w);check(l[1]==65,"overflow metadata");for(uint i=0;i<w[3];++i)check(close(rf::direct_listed(w.data(),l.data(),i),rf::direct(w.data(),i)),"overflow radiance");});
    test("local light is blocked by geometry",[]{auto s=basic();s.lights.push_back({v(0,0,2),v(1,1,1),10,.1f});roof(s);auto w=s.pack();no_environment(w);check(close(rf::direct(w.data(),0),v(0,0,0)),"local shadow");});
    test("one-bounce emission transport carries red",[]{auto s=basic();s.materials.push_back({v(.8f,.1f,.1f),v(2,0,0)});roof(s,1);auto w=s.pack();no_environment(w);V b=rf::indirect(w.data(),0);check(b.x>0&&b.y==0&&b.z==0,"emissive bounce");});
    test("nonemissive red wall produces diffuse color bleed",[]{
        auto s=basic();s.materials.push_back({v(.8f,.04f,.02f),v(0,0,0)});
        s.triangles.push_back({v(-100,2,0),v(100,2,0),v(100,2,100),1});
        s.triangles.push_back({v(-100,2,0),v(100,2,100),v(-100,2,100),1});
        auto w=s.pack();no_environment(w);put(w,16,rf::norm(v(0,-1,1)));put(w,20,v(1,1,1));
        V sum=v(0,0,0);for(uint i=1;i<=128;++i){w[13]=i;sum=add(sum,rf::indirect(w.data(),0));}
        check(sum.x>0&&sum.x>10*sum.y,"diffuse bounce missing");
    });
    test("sky bounce matches cosine-weighted irradiance",[]{auto s=basic();auto w=s.pack();put(w,24,v(1,1,1));V sum=v(0,0,0);for(uint i=0;i<16384;++i){w[13]=i;sum=add(sum,rf::indirect(w.data(),0));}V mean=mul(sum,1.0f/16384);check(close(mean.x,.5f/rf::PI,.004f),"sky estimator");});
    test("same frame gives same sample",[]{auto s=fixture();auto w=s.pack();auto a=rf::indirect(w.data(),4),b=rf::indirect(w.data(),4);check(a.x==b.x&&a.y==b.y&&a.z==b.z,"nondeterministic");});
    test("Fresnel and absorption limits",[]{check(close(rf::fresnel(1),.02037f)&&close(rf::fresnel(0),1),"Fresnel");V zero=rf::transmittance(v(.2f,.1f,.05f),0),thick=rf::transmittance(v(.2f,.1f,.05f),10);check(close(zero,v(1,1,1))&&thick.x<thick.y&&thick.y<thick.z&&thick.z<1,"absorption");});
    test("water only shades classified surfaces",[]{auto s=fixture();auto w=s.pack();check(close(rf::water(w.data(),0),v(0,0,0)),"water contamination");V c=rf::water(w.data(),w[3]-1);check(finite(c)&&c.x>=0&&c.y>=0&&c.z>=0,"water nonfinite");});
    test("water waves change with phase",[]{auto s=fixture();auto w=s.pack();V a=rf::water(w.data(),w[3]-1);put(w,35,2.1f);V b=rf::water(w.data(),w[3]-1);check(!close(a,b,1e-7f),"frozen water");});
    test("bad node links rejected before GPU",[]{auto s=fixture();auto w=s.pack();w[w[8]+10]=0;bool rejected=false;try{validate(w);}catch(const std::runtime_error&){rejected=true;}check(rejected,"bad escape admitted");});
    test("nonconservative leaf bounds rejected",[]{auto s=basic();roof(s);auto w=s.pack();put(w,w[8],v(0,0,1));bool rejected=false;try{validate(w);}catch(const std::runtime_error&){rejected=true;}check(rejected,"bad bound admitted");});
    test("nonfinite material rejected",[]{auto s=basic();auto w=s.pack();put(w,w[10],std::numeric_limits<float>::quiet_NaN());bool rejected=false;try{validate(w);}catch(const std::runtime_error&){rejected=true;}check(rejected,"NaN admitted");});
    test("out-of-range material rejected",[]{auto s=basic();auto w=s.pack();w[w[12]+3]=100;bool rejected=false;try{validate(w);}catch(const std::runtime_error&){rejected=true;}check(rejected,"bad material admitted");});
    std::cout<<"RESULT tests="<<tests<<" passed="<<tests<<" gpu_executed=0\n";
    if(argc==2){auto s=fixture();auto w=s.pack();validate(w);std::string prefix=argv[1];write_words(prefix+".scene.u32",w);auto l=lists(w);write_words(prefix+".lists.u32",l);
        for(const std::string stage:{"direct","indirect","water"}){std::vector<uint>out(w[3]*4);for(uint i=0;i<w[3];++i){V c=stage=="direct"?rf::direct_listed(w.data(),l.data(),i):(stage=="indirect"?rf::indirect(w.data(),i):rf::water(w.data(),i));put(out,i*4,c);put(out,i*4+3,1);}write_words(prefix+"."+stage+".f32",out);}std::cout<<"FIXTURE surfaces="<<w[3]<<" lights="<<w[7]<<" nodes="<<w[4]<<" words="<<w.size()<<'\n';}
    return 0;
}catch(const std::exception&e){std::cerr<<"FAIL after "<<tests<<" tests: "<<e.what()<<'\n';return 1;}}
