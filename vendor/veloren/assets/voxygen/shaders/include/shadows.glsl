#ifndef SHADOWS_GLSL
#define SHADOWS_GLSL

#ifdef HAS_SHADOW_MAPS
    #if (SHADOW_MODE == SHADOW_MODE_MAP)
        layout (std140, set = 0, binding = 9)
        uniform u_light_shadows {
            mat4 shadowMatrices;
            mat4 texture_mat;
        };
        
        // Use with sampler2DShadow
        layout(set = 1, binding = 2)
        uniform texture2D t_directed_shadow_maps;
        layout(set = 1, binding = 3)
        uniform samplerShadow s_directed_shadow_maps;
        
        // Use with samplerCubeShadow
        layout(set = 1, binding = 0)
        uniform textureCube t_point_shadow_maps;
        layout(set = 1, binding = 1)
        uniform samplerShadow s_point_shadow_maps;
        
        float VectorToDepth(vec3 Vec) {
            vec3 AbsVec = abs(Vec);
            float LocalZcomp = max(AbsVec.x, max(AbsVec.y, AbsVec.z));
        
            float NormZComp = shadow_proj_factors.x - shadow_proj_factors.y / LocalZcomp;
            return NormZComp;
        }
        
        const vec3 sampleOffsetDirections[20] = vec3[](
            vec3( 1,  1,  1), vec3( 1, -1,  1), vec3(-1, -1,  1), vec3(-1,  1,  1),
            vec3( 1,  1, -1), vec3( 1, -1, -1), vec3(-1, -1, -1), vec3(-1,  1, -1),
            vec3( 1,  1,  0), vec3( 1, -1,  0), vec3(-1, -1,  0), vec3(-1,  1,  0),
            vec3( 1,  0,  1), vec3(-1,  0,  1), vec3( 1,  0, -1), vec3(-1,  0, -1),
            vec3( 0,  1,  1), vec3( 0, -1,  1), vec3( 0, -1, -1), vec3( 0,  1, -1)
        );
        
        float ShadowCalculationPoint(uint lightIndex, vec3 fragToLight, vec3 fragNorm, vec3 fragPos) {
            if (lightIndex != 0u) {
                return 1.0;
            };
        
            float currentDepth = VectorToDepth(fragToLight);
        
            return textureGrad(samplerCubeShadow(t_point_shadow_maps, s_point_shadow_maps), vec4(fragToLight, currentDepth), vec3(0), vec3(0));
        }
        
        float ShadowCalculationDirected(in vec3 fragPos) {
            // Don't try to calculate directed shadows if there are no directed light sources
            // Applies, for example, in the char select menu
            if (light_shadow_count.z < 1) { return 1.0; }
        
            float bias = 0.0;
            float diskRadius = 0.01;
            vec4 sun_pos = texture_mat * vec4(fragPos, 1.0);
            return textureProj(sampler2DShadow(t_directed_shadow_maps, s_directed_shadow_maps), sun_pos);
        }
    #elif (SHADOW_MODE == SHADOW_MODE_NONE || SHADOW_MODE == SHADOW_MODE_CHEAP)
        float ShadowCalculationPoint(uint lightIndex, vec3 fragToLight, vec3 fragNorm, vec3 fragPos) {
            return 1.0;
        }
    #endif
#else
    float ShadowCalculationPoint(uint lightIndex, vec3 fragToLight, vec3 fragNorm, vec3 fragPos) {
        return 1.0;
    }
#endif

#endif
