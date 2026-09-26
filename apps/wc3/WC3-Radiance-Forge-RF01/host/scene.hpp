#pragma once
#define RF_HOST 1
#include "../kernels/rf_core.hpp"
#include <algorithm>
#include <fstream>
#include <stdexcept>
#include <string>
#include <vector>
#include <limits>
namespace rfhost {
using rf::V; using rf::v; using rf::add; using rf::sub; using rf::mul;
struct Material { V albedo=v(.5f,.5f,.5f), emission=v(0,0,0); };
struct Triangle { V a,b,c; uint material=0; };
struct Light { V position,intensity; float radius=20,softening=.1f; };
struct Surface { V p,n,tint=v(1,1,1); uint material=0,flags=rf::SURFACE_VALID; };
struct Node { V lo,hi; uint first=0,count=0,escape=0; };
inline V lower(V a,V b) { return v(std::min(a.x,b.x),std::min(a.y,b.y),std::min(a.z,b.z)); }
inline V upper(V a,V b) { return v(std::max(a.x,b.x),std::max(a.y,b.y),std::max(a.z,b.z)); }
inline void put(std::vector<uint>&w,uint i,float f) { std::memcpy(&w.at(i),&f,4); }
inline void put(std::vector<uint>&w,uint i,V f) { put(w,i,f.x);put(w,i+1,f.y);put(w,i+2,f.z); }
inline bool finite(V a) { return std::isfinite(a.x)&&std::isfinite(a.y)&&std::isfinite(a.z); }
inline bool inside(V p,V lo,V hi) { return p.x>=lo.x&&p.y>=lo.y&&p.z>=lo.z&&p.x<=hi.x&&p.y<=hi.y&&p.z<=hi.z; }
struct Scene {
    std::vector<Triangle> triangles;
    std::vector<Node> nodes;
    std::vector<Material> materials={{}};
    std::vector<Light> lights;
    std::vector<Surface> surfaces;
    uint build_node(uint begin,uint end) {
        uint index=uint(nodes.size()); nodes.push_back({});
        V lo=v(1e30f,1e30f,1e30f),hi=v(-1e30f,-1e30f,-1e30f);
        for(uint i=begin;i<end;++i) {
            const auto&t=triangles[i]; lo=lower(lo,lower(t.a,lower(t.b,t.c))); hi=upper(hi,upper(t.a,upper(t.b,t.c)));
        }
        nodes[index].lo=sub(lo,v(1e-5f,1e-5f,1e-5f)); nodes[index].hi=add(hi,v(1e-5f,1e-5f,1e-5f));
        if(end-begin<=4) { nodes[index].first=begin;nodes[index].count=end-begin; }
        else {
            V ext=sub(hi,lo); uint axis=ext.x>ext.y?(ext.x>ext.z?0:2):(ext.y>ext.z?1:2);
            uint mid=begin+(end-begin)/2;
            std::nth_element(triangles.begin()+begin,triangles.begin()+mid,triangles.begin()+end,[axis](const Triangle&a,const Triangle&b){
                return rf::axis(add(a.a,add(a.b,a.c)),axis)<rf::axis(add(b.a,add(b.b,b.c)),axis);
            });
            build_node(begin,mid); build_node(mid,end);
        }
        nodes[index].escape=uint(nodes.size()); return index;
    }
    std::vector<uint> pack() {
        // Bounded RF01 reference profile, not limits imposed by OpenGL.
        if(triangles.size()>1000000 || surfaces.size()>4000000 || lights.size()>4096 || materials.size()>65536 || materials.empty())
            throw std::runtime_error("RF01 reference profile capacity");
        nodes.clear(); if(!triangles.empty()) build_node(0,uint(triangles.size()));
        uint nb=rf::HEADER_WORDS,tb=nb+uint(nodes.size())*rf::NODE_WORDS;
        uint mb=tb+uint(triangles.size())*rf::TRI_WORDS, lb=mb+uint(materials.size())*rf::MAT_WORDS;
        uint sb=lb+uint(lights.size())*rf::LIGHT_WORDS, total=sb+uint(surfaces.size())*rf::SURFACE_WORDS;
        std::vector<uint>w(total,0); w[0]=rf::MAGIC;w[1]=rf::VERSION;w[2]=total;
        w[3]=uint(surfaces.size());w[4]=uint(nodes.size());w[5]=uint(triangles.size());w[6]=uint(materials.size());w[7]=uint(lights.size());
        w[8]=nb;w[9]=tb;w[10]=mb;w[11]=lb;w[12]=sb;
        put(w,16,rf::norm(v(.4f,.2f,1)));put(w,19,.00465f);put(w,20,v(3,2.8f,2.6f));put(w,23,.002f);
        put(w,24,v(.2f,.3f,.5f));put(w,27,1000);put(w,28,v(.15f,.06f,.025f));put(w,31,2);
        put(w,32,v(0,-5,4));put(w,35,0);put(w,36,1);put(w,37,.7f);put(w,38,.15f);
        for(uint i=0;i<nodes.size();++i) { uint a=nb+i*rf::NODE_WORDS; auto&n=nodes[i];put(w,a,n.lo);put(w,a+4,n.hi);w[a+8]=n.first;w[a+9]=n.count;w[a+10]=n.escape; }
        for(uint i=0;i<triangles.size();++i) { uint a=tb+i*rf::TRI_WORDS; auto&t=triangles[i];put(w,a,t.a);w[a+3]=t.material;put(w,a+4,sub(t.b,t.a));put(w,a+8,sub(t.c,t.a)); }
        for(uint i=0;i<materials.size();++i) { uint a=mb+i*rf::MAT_WORDS;put(w,a,materials[i].albedo);put(w,a+3,.6f);put(w,a+4,materials[i].emission); }
        for(uint i=0;i<lights.size();++i) { uint a=lb+i*rf::LIGHT_WORDS;auto&l=lights[i];put(w,a,l.position);put(w,a+3,l.radius);put(w,a+4,l.intensity);put(w,a+7,l.softening); }
        for(uint i=0;i<surfaces.size();++i) { uint a=sb+i*rf::SURFACE_WORDS;auto&p=surfaces[i];put(w,a,p.p);w[a+3]=p.material;put(w,a+4,p.n);w[a+7]=p.flags;put(w,a+8,p.tint);put(w,a+11,1);put(w,a+12,1);w[a+13]=i; }
        return w;
    }
};
// Strict host admission for this owned fixture file. Port into TRUEOS before accepting guest data.
inline void validate(const std::vector<uint>&w) {
    auto require=[](bool value,const char*message){ if(!value) throw std::runtime_error(message); };
    require(w.size()>=rf::HEADER_WORDS,"short header");
    require(w[0]==rf::MAGIC && w[1]==rf::VERSION && w[2]==w.size(),"magic/version/size");
    require(w[3]<=4000000 && w[4]<=2000000 && w[5]<=1000000 && w[6]>0 && w[6]<=65536 && w[7]<=4096,"capacity");
    std::uint64_t cursor=rf::HEADER_WORDS;
    const uint counts[5]={w[4],w[5],w[6],w[7],w[3]}, strides[5]={rf::NODE_WORDS,rf::TRI_WORDS,rf::MAT_WORDS,rf::LIGHT_WORDS,rf::SURFACE_WORDS};
    for(uint k=0;k<5;++k){ require(w[8+k]==cursor,"section offset");cursor+=std::uint64_t(counts[k])*strides[k];require(cursor<=w.size(),"section extent"); }
    require(cursor==w.size(),"trailing bytes");
    auto vecok=[&](uint a){ require(finite(rf::vec(w.data(),a)),"nonfinite vector"); };
    auto positive=[&](uint a){require(std::isfinite(rf::f(w.data(),a))&&rf::f(w.data(),a)>0,"nonpositive parameter");};
    for(uint a:{16u,20u,24u,28u,32u}) vecok(a);
    for(uint a:{19u,31u,35u,36u,37u,38u}) require(std::isfinite(rf::f(w.data(),a)),"nonfinite setting");
    positive(23);positive(27);require(rf::f(w.data(),19)>=0&&rf::f(w.data(),19)<=.1f,"sun angle");
    require(rf::f(w.data(),31)>=0&&rf::f(w.data(),38)>=.02f&&rf::f(w.data(),38)<=1,"water parameters");
    for(uint a:{20u,24u,28u}) {V c=rf::vec(w.data(),a);require(c.x>=0&&c.y>=0&&c.z>=0,"negative radiometric setting");}
    for(uint i=0;i<w[6];++i){ uint a=w[10]+i*rf::MAT_WORDS;vecok(a);vecok(a+4);V c=rf::vec(w.data(),a),e=rf::vec(w.data(),a+4);require(c.x>=0&&c.y>=0&&c.z>=0&&c.x<=1&&c.y<=1&&c.z<=1&&e.x>=0&&e.y>=0&&e.z>=0,"material values"); }
    for(uint i=0;i<w[5];++i){uint a=w[9]+i*rf::TRI_WORDS;vecok(a);vecok(a+4);vecok(a+8);require(w[a+3]<w[6],"triangle material");}
    for(uint i=0;i<w[7];++i){uint a=w[11]+i*rf::LIGHT_WORDS;vecok(a);vecok(a+4);positive(a+3);positive(a+7);V c=rf::vec(w.data(),a+4);require(c.x>=0&&c.y>=0&&c.z>=0,"negative light");}
    for(uint i=0;i<w[3];++i){uint a=w[12]+i*rf::SURFACE_WORDS;vecok(a);vecok(a+4);vecok(a+8);require(w[a+3]<w[6]&&!(w[a+7]&~3u),"surface metadata");require(std::isfinite(rf::f(w.data(),a+12))&&rf::f(w.data(),a+12)>=0,"surface emission");}
    require((w[4]==0)==(w[5]==0),"empty BVH mismatch");
    if(w[4]) require(w[w[8]+10]==w[4],"root escape");
    // Validate a binary preorder tree, its escape links, range coverage and conservative bounds.
    std::vector<uint> stack; uint next_triangle=0;
    for(uint i=0;i<w[4];++i){
        while(!stack.empty()&&i==w[w[8]+stack.back()*rf::NODE_WORDS+10]) stack.pop_back();
        uint a=w[8]+i*rf::NODE_WORDS;vecok(a);vecok(a+4);
        V lo=rf::vec(w.data(),a),hi=rf::vec(w.data(),a+4);require(inside(lo,lo,hi),"inverted bounds");
        require(w[a+10]>i && w[a+10]<=w[4],"escape link");
        if(i>0) require(!stack.empty(),"disconnected subtree");
        if(!stack.empty()){ uint parent=w[8]+stack.back()*rf::NODE_WORDS;require(w[a+10]<=w[parent+10],"escaping parent");require(inside(lo,rf::vec(w.data(),parent),rf::vec(w.data(),parent+4))&&inside(hi,rf::vec(w.data(),parent),rf::vec(w.data(),parent+4)),"nonconservative parent"); }
        if(w[a+9]==0){require(i+2<w[a+10],"empty interior");uint right=w[w[8]+(i+1)*rf::NODE_WORDS+10];require(right<w[a+10]&&w[w[8]+right*rf::NODE_WORDS+10]==w[a+10],"binary children");stack.push_back(i);}
        else {require(w[a+9]<=4&&w[a+8]==next_triangle&&std::uint64_t(next_triangle)+w[a+9]<=w[5]&&w[a+10]==i+1,"leaf range");
            for(uint j=0;j<w[a+9];++j){uint t=w[9]+(next_triangle+j)*rf::TRI_WORDS;V p=rf::vec(w.data(),t);require(inside(p,lo,hi)&&inside(add(p,rf::vec(w.data(),t+4)),lo,hi)&&inside(add(p,rf::vec(w.data(),t+8)),lo,hi),"nonconservative leaf");}
            next_triangle+=w[a+9]; }
    }
    require(next_triangle==w[5],"triangle coverage");
}
inline void write_words(const std::string&path,const std::vector<uint>&w){
    std::ofstream out(path,std::ios::binary);if(!out)throw std::runtime_error("open output");
    for(uint a:w){char b[4]={char(a),char(a>>8),char(a>>16),char(a>>24)};out.write(b,4);}if(!out)throw std::runtime_error("write output");
}
inline std::vector<uint> load_words(const std::string&path){
    std::ifstream in(path,std::ios::binary|std::ios::ate);if(!in)throw std::runtime_error("open input");auto end=in.tellg();
    if(end<0||std::uint64_t(end)>512ull*1024*1024||std::uint64_t(end)%4)throw std::runtime_error("file size");
    std::vector<unsigned char>b(static_cast<std::size_t>(end));
    in.seekg(0);in.read(reinterpret_cast<char*>(b.data()),std::streamsize(b.size()));if(!in)throw std::runtime_error("short read");
    std::vector<uint>w(b.size()/4);for(std::size_t i=0;i<w.size();++i)w[i]=uint(b[4*i])|(uint(b[4*i+1])<<8)|(uint(b[4*i+2])<<16)|(uint(b[4*i+3])<<24);return w;
}
} // namespace
