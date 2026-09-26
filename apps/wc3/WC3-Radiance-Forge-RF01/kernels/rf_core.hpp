#ifndef RF_CORE_HPP
#define RF_CORE_HPP
// Shared arithmetic and traversal for a host reference and C++ for OpenCL.
// No recursion, local traversal stack, allocation, images or samplers.
#ifdef RF_HOST
#include <cmath>
#include <cstdint>
#include <cstring>
using uint = std::uint32_t;
#define RF_GLOBAL
inline float rf_as_float(uint x) { float f; std::memcpy(&f,&x,4); return f; }
inline float rf_sqrt(float x) { return std::sqrt(x); }
inline float rf_sin(float x) { return std::sin(x); }
inline float rf_cos(float x) { return std::cos(x); }
inline float rf_exp(float x) { return std::exp(x); }
#else
#define RF_GLOBAL __global
inline float rf_as_float(uint x) { return as_float(x); }
inline float rf_sqrt(float x) { return sqrt(x); }
inline float rf_sin(float x) { return sin(x); }
inline float rf_cos(float x) { return cos(x); }
inline float rf_exp(float x) { return exp(x); }
#endif
namespace rf {
constexpr uint MAGIC=0x52463031u, VERSION=1u, HEADER_WORDS=64u;
constexpr uint NODE_WORDS=12u, TRI_WORDS=16u, MAT_WORDS=8u;
constexpr uint LIGHT_WORDS=8u, SURFACE_WORDS=16u;
constexpr uint SURFACES_PER_CLUSTER=64u, LIGHTS_PER_CLUSTER=32u, LIST_WORDS=34u;
constexpr uint INVALID=0xffffffffu, SURFACE_VALID=1u, SURFACE_WATER=2u;
constexpr float PI=3.14159265358979323846f;
// Header u32: magic,version,total words,nsurf,nnodes,ntri,nmat,nlights;
//             nodes,tris,mats,lights,surfaces,frame, reserved,reserved.
// Header f32: 16 sun direction.xyz,19 sun angle radians;
//             20 sun radiance.xyz,23 shadow bias;
//             24 sky irradiance.xyz,27 max ray length;
//             28 water absorption.xyz,31 water path length;
//             32 camera.xyz,35 water phase seconds; 36 wind.xy,38 roughness.
struct V { float x,y,z; };
inline V v(float x,float y,float z) { return {x,y,z}; }
inline V add(V a,V b) { return v(a.x+b.x,a.y+b.y,a.z+b.z); }
inline V sub(V a,V b) { return v(a.x-b.x,a.y-b.y,a.z-b.z); }
inline V mul(V a,float b) { return v(a.x*b,a.y*b,a.z*b); }
inline V had(V a,V b) { return v(a.x*b.x,a.y*b.y,a.z*b.z); }
inline float dot(V a,V b) { return a.x*b.x+a.y*b.y+a.z*b.z; }
inline V cross(V a,V b) { return v(a.y*b.z-a.z*b.y,a.z*b.x-a.x*b.z,a.x*b.y-a.y*b.x); }
inline float mn(float a,float b) { return a<b?a:b; }
inline float mx(float a,float b) { return a>b?a:b; }
inline float clamp(float x,float a,float b) { return mn(mx(x,a),b); }
inline float ab(float x) { return x<0?-x:x; }
inline V norm(V a) { float d=dot(a,a); return d>1e-20f?mul(a,1.0f/rf_sqrt(d)):v(0,0,1); }
inline float axis(V a,uint i) { return i==0?a.x:(i==1?a.y:a.z); }
inline float f(RF_GLOBAL const uint* s,uint i) { return rf_as_float(s[i]); }
inline V vec(RF_GLOBAL const uint* s,uint i) { return v(f(s,i),f(s,i+1),f(s,i+2)); }
inline uint mix(uint a) { a^=a>>16; a*=0x7feb352du; a^=a>>15; a*=0x846ca68bu; a^=a>>16; return a; }
inline float rnd(uint x) { return float(mix(x)>>8)*(1.0f/16777216.0f); }
inline V basis_sample(V n,float x,float y,float z) {
    V t=norm(cross(ab(n.z)<0.95f?v(0,0,1):v(0,1,0),n));
    return norm(add(add(mul(t,x),mul(cross(n,t),y)),mul(n,z)));
}
inline V cosine_direction(V n,uint seed) {
    float u=rnd(seed), phi=2.0f*PI*rnd(seed^0x928e1b51u), r=rf_sqrt(u);
    return basis_sample(n,r*rf_cos(phi),r*rf_sin(phi),rf_sqrt(mx(0,1-u)));
}
inline V sun_direction(RF_GLOBAL const uint* s,uint seed) {
    V d=norm(vec(s,16)); float r=rf_sqrt(rnd(seed))*f(s,19);
    float phi=2*PI*rnd(seed^0x8912abcdu);
    // Small-angle disk approximation; one new soft-shadow sample per frame.
    return basis_sample(d,r*rf_cos(phi),r*rf_sin(phi),1.0f);
}
struct Hit { float t; uint tri; };
inline bool box_hit(V o,V d,V lo,V hi,float tmax) {
    float near_t=0, far_t=tmax;
    for(uint a=0;a<3;++a) {
        float da=axis(d,a), oa=axis(o,a), la=axis(lo,a), ha=axis(hi,a);
        if(ab(da)<1e-20f) { if(oa<la || oa>ha) return false; }
        else {
            float t0=(la-oa)/da,t1=(ha-oa)/da;
            near_t=mx(near_t,mn(t0,t1)); far_t=mn(far_t,mx(t0,t1));
            if(far_t<near_t) return false;
        }
    }
    return far_t>=near_t;
}
inline float triangle_hit(RF_GLOBAL const uint* s,uint tid,V o,V d,float tmin,float tmax) {
    uint a=s[9]+tid*TRI_WORDS;
    V p=vec(s,a),e1=vec(s,a+4),e2=vec(s,a+8),q=cross(d,e2);
    float det=dot(e1,q); if(ab(det)<1e-8f) return tmax;
    float inv=1.0f/det; V t=sub(o,p); float u=dot(t,q)*inv;
    if(u<0 || u>1) return tmax;
    V r=cross(t,e1); float w=dot(d,r)*inv;
    if(w<0 || u+w>1) return tmax;
    float distance=dot(e2,r)*inv;
    return distance>tmin && distance<tmax?distance:tmax;
}
inline Hit trace(RF_GLOBAL const uint* s,V o,V d,float tmin,float tmax,bool any_hit) {
    Hit h={tmax,INVALID}; uint node=0;
    // Preorder threaded BVH: miss/leaf -> escape, interior hit -> next node.
    // Host admission MUST validate node links, ranges, finite bounds and containment.
    // Bounded by nnodes even for corrupted input; no recursion/private stack.
    for(uint visited=0; node<s[4] && visited<s[4]; ++visited) {
        uint a=s[8]+node*NODE_WORDS, first=s[a+8], count=s[a+9], escape=s[a+10];
        if(!box_hit(o,d,vec(s,a),vec(s,a+4),h.t)) { node=escape; continue; }
        if(count==0) { ++node; continue; }
        for(uint j=0;j<count;++j) {
            uint tid=first+j; float t=triangle_hit(s,tid,o,d,tmin,h.t);
            if(t<h.t) { h={t,tid}; if(any_hit) return h; }
        }
        node=escape;
    }
    return h;
}
inline bool visible(RF_GLOBAL const uint* s,V p,V n,V l,float max_t) {
    float bias=f(s,23);
    return trace(s,add(p,mul(n,bias)),l,bias*0.1f,max_t,true).tri==INVALID;
}
inline V sky_radiance(RF_GLOBAL const uint* s,V d) {
    // Analytic hemisphere: integral over a horizontal cosine lobe equals supplied sky irradiance.
    float factor=0.3f+1.05f*mx(0,d.z);
    return mul(vec(s,24),factor/PI);
}
inline V incident_selected(RF_GLOBAL const uint* s,V p,V n,uint seed,
                           RF_GLOBAL const uint* lists,uint list_base,uint count,bool use_list) {
    V l=sun_direction(s,seed), result=v(0,0,0);
    float nd=mx(0,dot(n,l));
    if(nd>0 && visible(s,p,n,l,f(s,27))) result=mul(vec(s,20),nd);
    // RF01 correctness baseline: all scene lights, NOT the GL light slots.
    // Replace by overflow-safe clustered lists only after matching this reference.
    for(uint j=0;j<count;++j) {
        uint i=use_list?lists[list_base+2+j]:j;
        uint a=s[11]+i*LIGHT_WORDS; V delta=sub(vec(s,a),p);
        float d2=dot(delta,delta), radius=f(s,a+3);
        if(d2<=1e-12f || d2>=radius*radius) continue;
        float dist=rf_sqrt(d2); V ld=mul(delta,1.0f/dist); float cosine=mx(0,dot(n,ld));
        if(cosine<=0) continue;
        float fade=mx(0,1.0f-(d2*d2)/(radius*radius*radius*radius));
        float min_d=f(s,a+7); // finite emitter regularization, not geometric emitter sampling
        float attenuation=fade*fade/mx(d2,min_d*min_d);
        if(visible(s,p,n,ld,mx(f(s,23),dist-f(s,23))))
            result=add(result,mul(vec(s,a+4),cosine*attenuation));
    }
    return result;
}
inline V incident_direct(RF_GLOBAL const uint* s,V p,V n,uint seed) {
    return incident_selected(s,p,n,seed,s,0,s[7],false);
}
inline V direct(RF_GLOBAL const uint* s,uint id) {
    uint a=s[12]+id*SURFACE_WORDS;
    if((s[a+7]&SURFACE_VALID)==0) return v(0,0,0);
    uint m=s[10]+s[a+3]*MAT_WORDS;
    V n=norm(vec(s,a+4)), base=had(vec(s,m),vec(s,a+8));
    V e=mul(vec(s,m+4),f(s,a+12));
    return add(e,mul(had(base,incident_direct(s,vec(s,a),n,mix(id)^s[13])),1.0f/PI));
}
inline V direct_listed(RF_GLOBAL const uint* s,RF_GLOBAL const uint* lists,uint id) {
    uint a=s[12]+id*SURFACE_WORDS;
    if((s[a+7]&SURFACE_VALID)==0) return v(0,0,0);
    uint m=s[10]+s[a+3]*MAT_WORDS, b=(id/SURFACES_PER_CLUSTER)*LIST_WORDS;
    bool use_list=lists[b+1]==0;
    uint count=use_list?lists[b]:s[7]; // NEVER drop overflow lights.
    V result=incident_selected(s,vec(s,a),norm(vec(s,a+4)),mix(id)^s[13],lists,b,count,use_list);
    return add(mul(vec(s,m+4),f(s,a+12)),mul(had(had(vec(s,m),vec(s,a+8)),result),1.0f/PI));
}
inline void build_light_list(RF_GLOBAL const uint* s,RF_GLOBAL uint* out,uint cluster) {
    uint b=cluster*LIST_WORDS, first=cluster*SURFACES_PER_CLUSTER;
    V lo=v(1e30f,1e30f,1e30f), hi=v(-1e30f,-1e30f,-1e30f); bool has_surface=false;
    for(uint k=0;k<SURFACES_PER_CLUSTER && first+k<s[3];++k) {
        uint a=s[12]+(first+k)*SURFACE_WORDS;
        if((s[a+7]&SURFACE_VALID)==0) continue;
        has_surface=true; V p=vec(s,a);
        lo=v(mn(lo.x,p.x),mn(lo.y,p.y),mn(lo.z,p.z));
        hi=v(mx(hi.x,p.x),mx(hi.y,p.y),mx(hi.z,p.z));
    }
    for(uint k=0;k<LIST_WORDS;++k) out[b+k]=0;
    if(!has_surface) return;
    uint accepted=0;
    for(uint i=0;i<s[7];++i) {
        uint a=s[11]+i*LIGHT_WORDS; V p=vec(s,a);
        V nearest=v(clamp(p.x,lo.x,hi.x),clamp(p.y,lo.y,hi.y),clamp(p.z,lo.z,hi.z));
        V delta=sub(p,nearest); float radius=f(s,a+3);
        if(dot(delta,delta)>radius*radius) continue;
        if(accepted<LIGHTS_PER_CLUSTER) out[b+2+accepted]=i;
        ++accepted;
    }
    out[b]=accepted<LIGHTS_PER_CLUSTER?accepted:LIGHTS_PER_CLUSTER;
    out[b+1]=accepted>LIGHTS_PER_CLUSTER?accepted:0;
}
inline V indirect(RF_GLOBAL const uint* s,uint id) {
    uint a=s[12]+id*SURFACE_WORDS;
    if((s[a+7]&SURFACE_VALID)==0) return v(0,0,0);
    uint m=s[10]+s[a+3]*MAT_WORDS;
    V p=vec(s,a),n=norm(vec(s,a+4));
    V base=had(vec(s,m),vec(s,a+8)); float bias=f(s,23);
    V rd=cosine_direction(n,mix(id)^(s[13]*0x9e3779b9u));
    V origin=add(p,mul(n,bias)); Hit hit=trace(s,origin,rd,bias*0.1f,f(s,27),false);
    V incoming;
    if(hit.tri==INVALID) incoming=sky_radiance(s,rd);
    else {
        uint t=s[9]+hit.tri*TRI_WORDS, hm=s[10]+s[t+3]*MAT_WORDS;
        V hn=norm(cross(vec(s,t+4),vec(s,t+8)));
        if(dot(hn,rd)>0) hn=mul(hn,-1); // RF01 opaque two-sided proxy geometry
        V hp=add(origin,mul(rd,hit.t));
        // One actual diffuse bounce, no recursive transport. Direct hit lighting casts its own shadow rays.
        incoming=add(vec(s,hm+4),mul(had(vec(s,hm),incident_direct(s,hp,hn,mix(id)^s[13]^123u)),1.0f/PI));
    }
    // Cosine sampling PDF cancels the Lambertian cosine/pi factor.
    return had(base,incoming);
}
inline float fresnel(float nv) { float q=1-clamp(nv,0,1), q2=q*q; return 0.02037f+0.97963f*q2*q2*q; }
inline V transmittance(V absorption,float distance) {
    return v(rf_exp(-absorption.x*distance),rf_exp(-absorption.y*distance),rf_exp(-absorption.z*distance));
}
inline V water(RF_GLOBAL const uint* s,uint id) {
    uint a=s[12]+id*SURFACE_WORDS;
    if((s[a+7]&(SURFACE_VALID|SURFACE_WATER))!=(SURFACE_VALID|SURFACE_WATER)) return v(0,0,0);
    V p=vec(s,a), view=norm(sub(vec(s,32),p));
    float phase=f(s,35), wx=f(s,36), wy=f(s,37), rough=f(s,38);
    // Two analytic wave slopes; zero tessellation required. This is an appearance replacement, not fluid simulation.
    V n=norm(v(-0.10f*rf_cos(p.x*1.7f+p.y*0.4f-phase*wx),
               -0.07f*rf_cos(p.y*2.1f-p.x*0.3f-phase*wy),1));
    if(dot(n,view)<0) n=mul(n,-1);
    V reflected=norm(sub(mul(n,2*dot(n,view)),view));
    Hit h=trace(s,add(p,mul(n,f(s,23))),reflected,f(s,23)*0.1f,f(s,27),false);
    V reflection;
    if(h.tri==INVALID) reflection=sky_radiance(s,reflected);
    else {
        uint t=s[9]+h.tri*TRI_WORDS, m=s[10]+s[t+3]*MAT_WORDS;
        V hn=norm(cross(vec(s,t+4),vec(s,t+8)));
        if(dot(hn,reflected)>0) hn=mul(hn,-1);
        V hp=add(add(p,mul(n,f(s,23))),mul(reflected,h.t));
        reflection=add(vec(s,m+4),mul(had(vec(s,m),incident_direct(s,hp,hn,mix(id)^s[13])),1/PI));
    }
    // RF01 under-water radiance is supplied in surface.rgb, already linear.
    // A later refraction pass produces it from opaque depth/color; do not use albedo here.
    V transmission=transmittance(vec(s,28),f(s,31));
    V through=had(vec(s,a+8),transmission);
    float F=fresnel(dot(n,view));
    V result=add(mul(reflection,F),mul(through,1-F));
    // Analytic sun disk lobe, visibility from the same scene. A full microfacet water BSDF is a later pass.
    V sun=norm(vec(s,16)); float align=mx(0,dot(reflected,sun));
    float power=1.0f; float exponent=clamp(2/(rough*rough)-2,2,256);
    // exp(log()) avoided: stable Gaussian approximation of the specular lobe.
    power=rf_exp((align-1)*exponent);
    if(dot(n,sun)>0 && visible(s,p,n,sun,f(s,27))) result=add(result,mul(vec(s,20),F*power));
    return result;
}
} // namespace rf
#endif
