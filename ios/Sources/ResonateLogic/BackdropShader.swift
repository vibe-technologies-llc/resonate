public enum BackdropShader {
    public static let vertexName = "backdropVertex"
    public static let fragmentName = "backdropFragment"
    public static let source = """
        #include <metal_stdlib>
        using namespace metal;

        constant float tau = 6.28318530718;
        constant int blobCount = 5;
        constant float2 blobPeriods[5] = {
            float2(36.0, 45.0), float2(40.0, 24.0), float2(30.0, 40.0), float2(45.0, 36.0), float2(24.0, 30.0)
        };
        constant float2 blobAnchors[5] = {
            float2(0.28, 0.22), float2(0.74, 0.34), float2(0.30, 0.68), float2(0.70, 0.82), float2(0.52, 0.50)
        };
        constant float2 blobPhases[5] = {
            float2(0.0, 1.3), float2(2.1, 0.4), float2(4.2, 3.3), float2(1.1, 5.0), float2(3.6, 2.2)
        };
        constant float2 blobReach = float2(0.26, 0.22);
        constant float blobBaseRadius = 0.38;
        constant float blobWeightRadius = 0.30;
        constant float blobBaseInfluence = 0.35;
        constant float groundInfluence = 0.03;
        constant float chromaKeeping = 0.6;
        constant float chromaRestoredAtMost = 2.5;
        constant float energyGrowth = 0.25;
        constant float broadWarp = 0.42;
        constant float fineWarp = 0.14;
        constant float contrast = 0.35;
        constant float vignetteDesaturation = 0.15;
        constant float ditherStep = 1.0 / 255.0;

        struct Field {
            float4 colours[5];
            float4 previous[5];
            float time;
            float energy;
            float progress;
            float aspect;
            float lightnessFloor;
            float lightnessRange;
            float chroma;
            float vignette;
        };

        struct Varyings {
            float4 position [[position]];
            float2 uv;
        };

        float hashed(float2 p) {
            float3 q = fract(float3(p.x, p.y, p.x) * 0.1031);
            q += dot(q, q.yzx + 33.33);
            return fract((q.x + q.y) * q.z);
        }

        float valueNoise(float2 p) {
            float2 cell = floor(p);
            float2 within = fract(p);
            float2 eased = within * within * within * (within * (within * 6.0 - 15.0) + 10.0);
            float a = hashed(cell);
            float b = hashed(cell + float2(1.0, 0.0));
            float c = hashed(cell + float2(0.0, 1.0));
            float d = hashed(cell + float2(1.0, 1.0));
            return mix(mix(a, b, eased.x), mix(c, d, eased.x), eased.y);
        }

        float3 linearFromOklab(float3 lab) {
            float3 root = float3(
                dot(lab, float3(1.0, 0.3963377774, 0.2158037573)),
                dot(lab, float3(1.0, -0.1055613458, -0.0638541728)),
                dot(lab, float3(1.0, -0.0894841775, -1.2914855480)));
            float3 lms = root * root * root;
            return float3(
                dot(lms, float3(4.0767416621, -3.3077115913, 0.2309699292)),
                dot(lms, float3(-1.2684380046, 2.6097574011, -0.3413193965)),
                dot(lms, float3(-0.0041960863, -0.7034186147, 1.7076147010)));
        }

        float2 blobCentre(int index, float time, float2 frame) {
            float2 angle = tau * time / blobPeriods[index] + blobPhases[index];
            float2 wander = float2(sin(angle.x), cos(angle.y)) * blobReach;
            return (blobAnchors[index] + wander - 0.5) * frame;
        }

        float3 fieldOf(constant float4 *labs, float2 place, float time, float energy, float2 frame) {
            float growth = 1.0 + energyGrowth * energy;
            float3 blended = float3(0.0);
            float3 ground = float3(0.0);
            float blendedChroma = 0.0;
            float groundChroma = 0.0;
            float total = 0.0;
            float weights = 0.0;
            for (int index = 0; index < blobCount; index++) {
                float4 colour = labs[index];
                float3 lab = colour.xyz;
                float chroma = length(lab.yz);
                float2 offset = place - blobCentre(index, time, frame);
                float radius = (blobBaseRadius + blobWeightRadius * sqrt(colour.w)) * growth;
                float influence = (blobBaseInfluence + colour.w) * exp(-dot(offset, offset) / (radius * radius));
                blended += influence * lab;
                blendedChroma += influence * chroma;
                total += influence;
                ground += colour.w * lab;
                groundChroma += colour.w * chroma;
                weights += colour.w;
            }
            float share = max(total + groundInfluence * weights, 0.0001);
            float3 mixed = (blended + groundInfluence * ground) / share;
            float meanChroma = (blendedChroma + groundInfluence * groundChroma) / share;
            float mixedChroma = length(mixed.yz);
            float wantedChroma = mix(mixedChroma, meanChroma, chromaKeeping);
            float restored = min(wantedChroma / max(mixedChroma, 0.02), chromaRestoredAtMost);
            return float3(mixed.x, mixed.yz * restored);
        }

        float2 warped(float2 place, float time, float energy) {
            float2 drift = float2(cos(tau * time / 120.0), sin(tau * time / 120.0)) * 1.6;
            float2 eddy = float2(sin(tau * time / 90.0), cos(tau * time / 90.0)) * 2.4;
            float2 broad = place * 1.25 + drift;
            float2 fine = place * 2.7 - eddy;
            float2 first = float2(valueNoise(broad), valueNoise(broad + float2(5.2, 1.3))) - 0.5;
            float2 second = float2(valueNoise(fine + float2(8.3, 2.8)), valueNoise(fine + float2(1.7, 9.2))) - 0.5;
            return place + (first * broadWarp + second * fineWarp) * (1.0 + energyGrowth * energy);
        }

        float3 shaped(float3 lab, float2 uv, constant Field &field) {
            float lightness = saturate(lab.x);
            float curved = mix(lightness, lightness * lightness * (3.0 - 2.0 * lightness), contrast);
            float edge = smoothstep(0.30, 0.95, length((uv - 0.5) * 2.0) * 0.7071);
            float darkened = (field.lightnessFloor + field.lightnessRange * curved) * (1.0 - field.vignette * edge);
            float kept = sqrt(saturate(darkened / max(lightness, 0.05)));
            float chroma = field.chroma * kept * (1.0 - vignetteDesaturation * edge);
            return float3(darkened, lab.yz * chroma);
        }

        float3 srgbEncoded(float3 rgb) {
            float3 low = rgb * 12.92;
            float3 high = 1.055 * powr(max(rgb, float3(0.0031308)), float3(1.0 / 2.4)) - 0.055;
            return select(high, low, rgb <= float3(0.0031308));
        }

        vertex Varyings backdropVertex(uint vertexID [[vertex_id]]) {
            float2 corner = float2(float((vertexID << 1) & 2), float(vertexID & 2));
            Varyings out;
            out.position = float4(corner * 2.0 - 1.0, 0.0, 1.0);
            out.uv = float2(corner.x, 1.0 - corner.y);
            return out;
        }

        fragment float4 backdropFragment(Varyings in [[stage_in]], constant Field &field [[buffer(0)]]) {
            float aspect = max(field.aspect, 0.01);
            float2 frame = float2(max(aspect, 1.0), max(1.0 / aspect, 1.0));
            float2 place = warped((in.uv - 0.5) * frame, field.time, field.energy);
            float3 lab = fieldOf(field.colours, place, field.time, field.energy, frame);
            if (field.progress < 1.0) {
                float3 before = fieldOf(field.previous, place, field.time, field.energy, frame);
                lab = mix(before, lab, smoothstep(0.0, 1.0, field.progress));
            }
            float3 rgb = saturate(linearFromOklab(shaped(lab, in.uv, field)));
            float dither = (hashed(in.position.xy) + hashed(in.position.xy + float2(17.0, 59.0)) - 1.0) * ditherStep;
            return float4(srgbEncoded(rgb) + dither, 1.0);
        }
        """
}
