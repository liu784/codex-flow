cbuffer FrameConstants : register(b0)
{
    float2 captureSize;
    float2 windowOrigin;
    float2 monitorOrigin;
    float2 outputSize;
    float percentage;
    float health;
    float hasPercentage;
    float dpiScale;
    float elapsedSeconds;
    float3 framePadding;
};

Texture2D desktopTexture : register(t0);
Texture2D<float> textAtlas : register(t1);
Texture2D shellTexture : register(t2);
SamplerState desktopSampler : register(s0);

struct VertexOutput
{
    float4 position : SV_POSITION;
    float2 uv : TEXCOORD0;
};

VertexOutput VSMain(uint vertexId : SV_VertexID)
{
    VertexOutput result;
    float2 uv = float2((vertexId << 1) & 2, vertexId & 2);
    result.position = float4(uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    result.uv = uv;
    return result;
}

// Atlas cells contain DirectWrite Segoe UI coverage, generated once at startup.
// Sampling is in screen coordinates, after the complete optical composition.
float valueMask(float2 pixel)
{
    int value = hasPercentage > 0.5 ? clamp((int)round(percentage), 0, 100) : 101;
    float2 cell = pixel - outputSize * 0.5 + float2(32.0, 16.0);
    if (any(cell < 0.5) || any(cell > float2(63.5, 31.5))) return 0.0;
    float2 origin = float2(value % 8, value / 8) * float2(64.0, 32.0);
    return textAtlas.SampleLevel(desktopSampler, (origin + cell) / float2(512.0, 416.0), 0);
}

float3 transmitted(float2 pixel, float2 dispersion)
{
    float3 color;
    color.r = desktopTexture.Sample(desktopSampler, (pixel + dispersion) / captureSize).r;
    color.g = desktopTexture.Sample(desktopSampler, pixel / captureSize).g;
    color.b = desktopTexture.Sample(desktopSampler, (pixel - dispersion) / captureSize).b;
    return color;
}

float3 healthColor(float healthValue)
{
    if (healthValue < 0.5) return float3(0.80, 0.91, 0.94);
    if (healthValue < 1.5) return float3(0.02, 0.82, 1.00);
    if (healthValue < 2.5) return float3(1.00, 0.76, 0.08);
    return float3(1.00, 0.20, 0.055);
}

float healthEnergy(float healthValue)
{
    if (healthValue < 1.5) return 1.00;
    if (healthValue < 2.5) return 1.04;
    return 1.08;
}

float3 healthBodyHighlight(float healthValue, float3 accent)
{
    if (healthValue < 1.5) return lerp(accent, float3(1.0, 1.0, 1.0), 0.46);
    if (healthValue < 2.5) return lerp(accent, float3(1.0, 1.0, 0.86), 0.42);
    return lerp(accent, float3(1.0, 0.82, 0.38), 0.30);
}

float3 healthCoreColor(float healthValue, float3 accent)
{
    if (healthValue < 1.5) return lerp(accent, float3(1.0, 1.0, 1.0), 0.84);
    if (healthValue < 2.5) return lerp(accent, float3(1.0, 1.0, 0.82), 0.78);
    return lerp(accent, float3(1.0, 0.78, 0.32), 0.66);
}

float4 over(float4 below, float3 color, float alpha)
{
    float4 above = float4(color * alpha, alpha);
    return above + below * (1.0 - above.a);
}

float directional(float2 direction, float2 lightDirection, float power)
{
    return pow(saturate(dot(direction, normalize(lightDirection))), power);
}

// One analytic arc, sampled with the existing frame clock; no particles/segments.
float headPhase(float time)
{
    return min(frac(time / 2.4) / 0.90, 1.0);
}

float trailAlpha(float remaining)
{
    return 0.62 * pow(saturate(remaining), 1.15);
}

float3 orbitMasks(float2 localPoint, float orbitRadius, float time)
{
    if (time <= 0.0) return 0.0;
    float theta = headPhase(time) * 6.28318530718;
    float pathRadius = orbitRadius + 0.4;
    float2 head = float2(sin(theta), -cos(theta)) * pathRadius;
    float headDistance = length(localPoint - head);
    float core = 1.0 - smoothstep(1.75, 2.15, headDistance);
    float glow = pow(1.0 - saturate(headDistance / 6.5), 2.0) * 0.22;
    float angle = atan2(localPoint.x, -localPoint.y);
    float behind = frac((theta - angle) / 6.28318530718) * 6.28318530718;
    const float trailAngle = 0.628318530718; // 36 degrees
    float remaining = saturate(1.0 - behind / trailAngle);
    float halfWidth = lerp(0.5, 2.4, remaining) * 0.5;
    float radialDistance = abs(length(localPoint) - pathRadius);
    float coverage = 1.0 - smoothstep(max(0.0, halfWidth - 0.30),
        halfWidth + 0.30, radialDistance);
    float trail = coverage * trailAlpha(remaining);
    float outsideText = smoothstep(17.0, 20.0, length(localPoint));
    return float3(core, glow * outsideText, trail);
}

float4 PSMain(VertexOutput input) : SV_TARGET
{
    float2 localPixel = input.uv * outputSize;
    float2 center = outputSize * 0.5;
    float2 p = (localPixel - center) / 40.0;
    float r2 = dot(p, p);
    if (r2 > 1.0) discard;

    // Locked D / LIQUID_SPHERICAL_GLASS geometry. Material layers below may
    // change local light response, but never this rotationally symmetric field.
    float radius = sqrt(r2);
    float z = sqrt(max(0.0, 1.0 - r2));
    float3 sphereNormal = normalize(float3(p, z));
    float3 incident = float3(0.0, 0.0, -1.0);
    float3 refracted = refract(incident, sphereNormal, 1.0 / 1.46);
    float2 desktopCenter = windowOrigin - monitorOrigin + center * dpiScale;
    float2 samplePixel = desktopCenter + (localPixel - center
        + refracted.xy * (25.5 * z * radius)) * dpiScale;

    float2 direction = radius > 0.0001 ? p / radius : float2(0.0, -1.0);
    float2 tangent = float2(-direction.y, direction.x);
    float upperLeft = directional(direction, float2(-0.70, -0.71), 5.5);
    float rightCaustic = directional(direction, float2(0.91, 0.41), 10.0);
    float warmPocket = directional(direction, float2(-0.76, 0.65), 18.0);
    float depthPocket = directional(direction, float2(0.94, 0.34), 5.0);

    // The live desktop remains the optical ground truth. The static asset is
    // only a restrained shell-highlight overlay; it never replaces transmission.
    float3 glass = transmitted(samplePixel, direction * 0.08 * radius * dpiScale);
    float alpha = 0.998 * (1.0 - smoothstep(0.986, 1.0, radius));
    float4 result = float4(saturate(glass) * alpha, alpha);

    // The source asset has a transparent center and contains only shell optics.
    // A small fixed lift plus a continuous light-background compensation affects
    // only the premultiplied shell overlay, preserving center transmission.
    float2 shellUv = float2(0.5000, 0.4992) + p * 0.4380;
    float4 shellOverlay = shellTexture.SampleLevel(desktopSampler, shellUv, 0);
    float transmittedLuma = dot(glass, float3(0.2126, 0.7152, 0.0722));
    float lightBackgroundCompensation = lerp(
        1.0, 1.05, smoothstep(0.62, 0.90, transmittedLuma));
    float shellOverlayStrength = saturate(
        (0.30 + 0.13 * upperLeft + 0.08 * rightCaustic + 0.05 * warmPocket)
        * 1.10 * lightBackgroundCompensation);
    // Dim only the outer decoration; leave ring/text region unchanged.
    float outerDecoration = lerp(1.0, 0.65, smoothstep(30.0, 34.0, radius * 40.0));
    shellOverlay *= shellOverlayStrength * outerDecoration;
    result = shellOverlay + result * (1.0 - shellOverlay.a);

    // The base and active progress states share one procedural glass-tube body.
    // The shell overlay carries no ring geometry.
    float ringRadius = 24.5;
    float tubeRadius = 2.75;
    float r = radius * 40.0;
    float signedDistance = r - ringRadius;
    float distance = abs(signedDistance);
    float annulus = 1.0 - smoothstep(2.35, 2.78, distance);
    float section = saturate(1.0 - signedDistance * signedDistance
        / (tubeRadius * tubeRadius));
    float tubeHeight = sqrt(section);
    float tubeNormal = clamp(signedDistance / tubeRadius, -1.0, 1.0);
    float2 channelPixel = samplePixel - direction * signedDistance
        * (0.50 + 0.34 * tubeHeight) * dpiScale
        + tangent * (0.18 * tubeHeight * upperLeft * dpiScale);
    float channelChroma = 0.24 * abs(tubeNormal)
        * (0.14 + rightCaustic + 0.45 * warmPocket);
    float3 channel = transmitted(channelPixel, direction * channelChroma * dpiScale);

    float tubeFacing = dot(direction, normalize(float2(-0.68, -0.73)));
    float tubeLight = saturate(0.64 * tubeHeight - 0.70 * tubeNormal * tubeFacing);
    float localReflection = pow(tubeLight, 4.0)
        * (0.12 + 0.25 * upperLeft);
    float localDepth = tubeHeight * (0.030 + 0.050 * depthPocket)
        * saturate(0.25 + 0.75 * tubeNormal);
    float channelLuma = dot(channel, float3(0.2126, 0.7152, 0.0722));
    float channelLift = 0.32 + 0.24 * saturate(0.62 - channelLuma);
    channel *= 1.0 - localDepth;
    channel = lerp(channel, float3(0.77, 0.94, 1.0), channelLift * tubeHeight);
    channel = lerp(channel, float3(0.58, 0.84, 0.96),
        0.075 * tubeHeight * (0.35 + 0.65 * depthPocket));
    channel = lerp(channel, float3(1.0, 0.998, 0.985),
        localReflection + 0.14 * abs(tubeNormal) * (0.35 + 0.65 * upperLeft));
    channel = lerp(channel, float3(0.63, 0.94, 1.0),
        0.070 * tubeHeight * rightCaustic);
    channel = lerp(channel, float3(1.0, 0.85, 0.70),
        0.032 * tubeHeight * warmPocket);
    result.rgb = lerp(result.rgb, channel * result.a, annulus);

    float progress = hasPercentage > 0.5 ? saturate(percentage / 100.0) : 0.0;
    float2 v = localPixel - center;
    float angle = atan2(v.x, -v.y);
    if (angle < 0.0) angle += 6.28318530718;
    float sweep = 6.28318530718 * progress;
    // Keep the real progress ring at its original non-animated appearance.
    float emissiveIntensity = 0.72;
    float bloomIntensity = 0.72;
    float bodyIntensity = 0.94;
    float active = 0.0;
    float activeCore = 0.0;
    float bloom = 0.0;
    float capHighlight = 0.0;
    if (progress > 0.0)
    {
        // Round ends stay within the actual sweep, so a small high-percentage gap
        // remains unlit rather than being covered by two protruding cap circles.
        float capRadius = min(tubeRadius, ringRadius * sin(min(sweep * 0.5, 1.57079632679)));
        float capAngle = asin(capRadius / ringRadius);
        float2 start = float2(sin(capAngle), -cos(capAngle)) * ringRadius;
        float2 end = float2(sin(sweep - capAngle), -cos(sweep - capAngle)) * ringRadius;
        float capDistance = min(length(v - start), length(v - end));
        bool onArc = angle >= capAngle && angle <= sweep - capAngle;
        float segmentDistance = onArc ? distance : capDistance + tubeRadius - capRadius;
        if (progress >= 0.99999) segmentDistance = distance;
        active = 1.0 - smoothstep(2.35, 2.78, segmentDistance);
        activeCore = 1.0 - smoothstep(0.50, 1.50, segmentDistance);
        bloom = (1.0 - smoothstep(2.75, 4.00, segmentDistance))
            * (1.0 - active * 0.70);
        capHighlight = (1.0 - smoothstep(
            tubeRadius * 0.30, tubeRadius * 1.05, capDistance)) * active;
    }
    float3 accent = healthColor(health);
    float stateEnergy = healthEnergy(health);
    float3 bodyHighlight = healthBodyHighlight(health, accent);
    float3 coreColor = healthCoreColor(health, accent);
    float lighting = 0.42 + 0.58
        * directional(direction, float2(-0.30, -0.95), 2.0);
    float3 activeBody = channel * lerp(float3(1.0, 1.0, 1.0), accent,
        0.72 + 0.15 * tubeHeight);
    activeBody = lerp(activeBody, bodyHighlight,
        0.58 * tubeHeight * emissiveIntensity);
    activeBody *= bodyIntensity * 1.10 * stateEnergy;
    result.rgb = lerp(result.rgb, saturate(activeBody) * result.a, active * 0.995);
    result = over(result, lerp(accent, float3(1.0, 1.0, 1.0), 0.36),
        bloom * 0.125 * 1.24 * stateEnergy * bloomIntensity);
    result = over(result, coreColor,
        activeCore * saturate(
            (0.48 + 0.38 * lighting) * 1.22 * stateEnergy * emissiveIntensity));
    result = over(result, coreColor,
        capHighlight * 0.10 * stateEnergy * emissiveIntensity);
    result = over(result, float3(1.0, 0.998, 0.985),
        active * localReflection * 0.80);

    // Independent same-hue overlay, not another modulation of the ring stroke.
    if (progress > 0.0)
    {
        float3 masks = orbitMasks(v, ringRadius, elapsedSeconds);
        result = over(result, accent, masks.y);
        result = over(result, accent, masks.z);
        result = over(result, accent, masks.x);
    }

    // Crisp, screen-space information sits above the optical layers.
    float text = valueMask(localPixel);
    float edgeCardinal = max(max(valueMask(localPixel + float2(0.60, 0.0)),
        valueMask(localPixel - float2(0.60, 0.0))),
        max(valueMask(localPixel + float2(0.0, 0.60)),
            valueMask(localPixel - float2(0.0, 0.60))));
    float edgeDiagonal = max(max(valueMask(localPixel + float2(0.44, 0.44)),
        valueMask(localPixel + float2(-0.44, 0.44))),
        max(valueMask(localPixel + float2(0.44, -0.44)),
            valueMask(localPixel - float2(0.44, 0.44))));
    float edgeContrast = saturate(max(edgeCardinal, edgeDiagonal * 0.86) - text * 0.78);
    float shadowCoverage = valueMask(localPixel - float2(0.0, 0.78));
    float microShadow = saturate(shadowCoverage - text * 0.72);
    float surfaceLuma = dot(result.rgb / max(result.a, 0.001),
        float3(0.2126, 0.7152, 0.0722));
    float lightBackground = smoothstep(0.58, 0.90, surfaceLuma);
    result = over(result, float3(0.040, 0.055, 0.070),
        edgeContrast * lerp(0.20, 0.35, lightBackground));
    result = over(result, float3(0.020, 0.030, 0.045),
        microShadow * lerp(0.12, 0.22, lightBackground));
    result = over(result, float3(1.0, 1.0, 1.0), text);
    return result;
}
