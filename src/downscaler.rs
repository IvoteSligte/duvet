use tracing::debug;
use windows::{
    Win32::Graphics::{Direct3D::Fxc::D3DCompile, Direct3D11::*, Dxgi::Common::DXGI_SAMPLE_DESC},
    core::{PCSTR, Result},
};

fn create_resized_texture(
    device: &ID3D11Device,
    src_texture: &ID3D11Texture2D,
    target_resolution: (u32, u32),
) -> Result<ID3D11Texture2D> {
    debug!("Creating destination texture");
    let (target_width, target_height) = target_resolution;
    unsafe {
        let mut src_desc = D3D11_TEXTURE2D_DESC::default();
        src_texture.GetDesc(&mut src_desc);
        assert_eq!(src_desc.MipLevels, 1);
        assert_eq!(src_desc.SampleDesc.Count, 1);
        assert!(src_desc.BindFlags & D3D11_BIND_SHADER_RESOURCE.0 as u32 != 0);

        let desc = D3D11_TEXTURE2D_DESC {
            Width: target_width,
            Height: target_height,
            MipLevels: 1,
            ArraySize: 1,
            Format: src_desc.Format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_UNORDERED_ACCESS).0 as _,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut texture = None;
        device.CreateTexture2D(&desc, None, Some(&mut texture))?;
        let texture = texture.unwrap();
        Ok(texture)
    }
}

fn create_shader(device: &ID3D11Device) -> Result<ID3D11ComputeShader> {
    debug!("Creating compute shader");
    let source = b"
Texture2D<float4> SrcTexture : register(t0);
RWTexture2D<float4> DstTexture : register(u0);
SamplerState LinearSampler : register(s0);

[numthreads(8, 8, 1)]
void CSMain(uint3 id : SV_DispatchThreadID)
{
    uint width, height;
    DstTexture.GetDimensions(width, height);

    if (id.x >= width || id.y >= height) {
        return;
    }
    float2 uv = (float2(id.xy) + 0.5f) / float2(width, height);
    DstTexture[id.xy] = SrcTexture.SampleLevel(LinearSampler, uv, 0);
}
";
    unsafe {
        let mut bytecode_blob = None;
        D3DCompile(
            source.as_ptr() as _,
            source.len(),
            None,
            None,
            None,
            PCSTR(b"CSMain\0" as _), // entrypoint
            PCSTR(b"cs_5_0\0" as _), // compilation target
            0,
            0,
            &mut bytecode_blob,
            None,
        )?;
        let bytecode_blob = bytecode_blob.unwrap();
        let bytecode = std::slice::from_raw_parts(
            bytecode_blob.GetBufferPointer() as _,
            bytecode_blob.GetBufferSize(),
        );
        let mut shader = None;
        device.CreateComputeShader(bytecode, None, Some(&mut shader))?;
        Ok(shader.unwrap())
    }
}

fn create_sampler(device: &ID3D11Device) -> Result<ID3D11SamplerState> {
    debug!("Creating sampler");
    unsafe {
        let desc = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT,
            AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
            MipLODBias: 0.0,
            MaxAnisotropy: 1,
            ComparisonFunc: D3D11_COMPARISON_ALWAYS,
            BorderColor: [0.0; 4],
            MinLOD: 0.0,
            MaxLOD: 0.0,
        };
        let mut sampler = None;
        device.CreateSamplerState(&desc, Some(&mut sampler))?;
        Ok(sampler.unwrap())
    }
}

fn create_shader_resource_view(
    device: &ID3D11Device,
    texture: &ID3D11Texture2D,
) -> Result<ID3D11ShaderResourceView> {
    debug!("Creating shader resource view");
    unsafe {
        let mut view = None;
        device.CreateShaderResourceView(texture, None, Some(&mut view))?;
        Ok(view.unwrap())
    }
}

fn create_unordered_access_view(
    device: &ID3D11Device,
    texture: &ID3D11Texture2D,
) -> Result<ID3D11UnorderedAccessView> {
    debug!("Creating unordered access view");
    unsafe {
        let mut view = None;
        device.CreateUnorderedAccessView(texture, None, Some(&mut view))?;
        Ok(view.unwrap())
    }
}

// TODO: optimize
/// Makes a few assumptions about `src_texture`:
/// It must have a `float4`-compatible format.
/// It may not be multi-sampled or have multiple MIP-levels.
/// It must have the `D3D11_BIND_SHADER_RESOURCE` bind flag.
pub fn downscale(
    device: &ID3D11Device,
    device_context: &ID3D11DeviceContext,
    src_texture: &ID3D11Texture2D,
    target_resolution: (u32, u32),
) -> Result<ID3D11Texture2D> {
    let (width, height) = target_resolution;
    debug!("Downscaling texture to {width}x{height}");

    let dst_texture = create_resized_texture(device, src_texture, target_resolution)?;
    let shader = create_shader(device)?;
    let sampler = create_sampler(device)?;
    unsafe {
        let src_view = create_shader_resource_view(device, src_texture)?;
        let dst_view = create_unordered_access_view(device, &dst_texture)?;

        device_context.CSSetShader(&shader, None);
        device_context.CSSetSamplers(0, Some(&[Some(sampler)]));
        device_context.CSSetShaderResources(0, Some(&[Some(src_view)]));
        device_context.CSSetUnorderedAccessViews(0, 1, Some(&Some(dst_view)), None);
        device_context.Dispatch(width.div_ceil(8), height.div_ceil(8), 1);
    }
    Ok(dst_texture)
}
