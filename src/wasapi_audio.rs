use crate::fft::time_domain_to_frequency_domain;
use crate::SharedBuffer;
use rustfft::FftPlanner;
use std::ptr::null_mut;
use std::sync::{Arc, Mutex};
use windows::core::GUID;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::StructuredStorage::{PropVariantToString, PROPVARIANT};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};
use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
pub const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
pub const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
pub const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;

pub fn start_desktop_audio_capture(buffer: SharedBuffer, ffi_enabled: Arc<Mutex<bool>>) -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        let mut accumulation_buffer: Vec<f32> = Vec::with_capacity(4096);
        let mut planner = FftPlanner::new();
        CoInitializeEx(None, COINIT_MULTITHREADED)?;

        let devname_pkey = PROPERTYKEY {fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 14}; 
        // ^devices pkey, since i couldn't find it in the crate

        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).unwrap_or_else(|_| {
            let avail_devices = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE).unwrap();
            avail_devices.Item(0).unwrap()
        });

        let mut device_pvar_name = device.OpenPropertyStore(STGM_READ)?.GetValue(&devname_pkey)?;
        let propvar_pntr = &mut device_pvar_name as *mut PROPVARIANT;
        let mut name_buffer: [u16; 256] = [0; 256];
        PropVariantToString(propvar_pntr, &mut name_buffer)?;
        let dev_name = String::from_utf16
        (
            &name_buffer[..name_buffer.iter()
            .position(|&x| x == 0)
            .unwrap_or(name_buffer.len())
            ]
        )?;

        let collection = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        println!("available devices:");
        for i in 0..collection.GetCount()? {
            let item = collection.Item(i)?;
            let mut pvar = item.OpenPropertyStore(STGM_READ)?.GetValue(&devname_pkey)?;
            let pntr = &mut pvar as *mut PROPVARIANT;
            let mut psz: [u16; 256] = [0; 256];
            PropVariantToString(pntr, &mut psz)?;
            println!("{:?}", String::from_utf16(&psz[..psz.iter().position(|&x| x == 0).unwrap_or(psz.len())])?);
        }

        let audio_client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;

        let wave_format = audio_client.GetMixFormat()?;
        let mut closest_match: *mut WAVEFORMATEX = null_mut();
        let result = audio_client.IsFormatSupported(
            AUDCLNT_SHAREMODE_SHARED,
            wave_format,
            Some(&mut closest_match),
        );

        let final_format = if result.is_ok() {
            wave_format
        } else if !closest_match.is_null() {
            closest_match
        } else {
            wave_format
        };

        let format_tag = (*final_format).wFormatTag;
        let bits = (*final_format).wBitsPerSample;
        let is_float = if format_tag == WAVE_FORMAT_EXTENSIBLE as u16 {
            let ext = final_format as *const WAVEFORMATEXTENSIBLE;
            let sub_format = std::ptr::read_unaligned(std::ptr::addr_of!((*ext).SubFormat));
            sub_format == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
        } else {
            format_tag == WAVE_FORMAT_IEEE_FLOAT as u16
        };


        audio_client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            10_000_000, // 100 ms buffer (a constant like this works properly and if i use the device period the image looks awful)
            0,
            final_format,
            None,
        )?;

        let capture_client: IAudioCaptureClient = audio_client.GetService()?;

        audio_client.Start()?;

        println!("\ndesktop audio capture started successfully on device: {}\n", dev_name);

        let mut last_ffi_enabled = false;
        let mut required_elements;

        loop {
            std::thread::sleep(std::time::Duration::from_millis(10));

            let packet_length = capture_client.GetNextPacketSize()?;

            if packet_length > 0 {
                let mut data_ptr = null_mut();
                let mut num_frames = 0u32;
                let mut flags = 0u32;

                capture_client.GetBuffer(
                    &mut data_ptr,
                    &mut num_frames,
                    &mut flags,
                    None,
                    None,
                )?;

                if num_frames > 0 && !data_ptr.is_null() {
                    let channels = (*wave_format).nChannels as usize;
                    let sample_count = (num_frames as usize) * channels;
                    //let samples = std::slice::from_raw_parts(data_ptr as *const f32, sample_count);
                    let converted = samples_to_f32(data_ptr as *const u8, sample_count, bits, is_float);
                    let channels = (*final_format).nChannels as usize;
                    for chunk in converted.chunks_exact(channels) {
                        let mono = chunk.iter().sum::<f32>() / channels as f32;
                        accumulation_buffer.push(mono);
                    }

                    if let Ok(mut buffer_guard) = buffer.try_lock() {
                        if last_ffi_enabled {
                            required_elements = 1536;
                        } else {
                            required_elements = 1024;
                        }
                        if accumulation_buffer.len() >= required_elements {
                            buffer_guard.clear();
                            buffer_guard.extend_from_slice(&accumulation_buffer[..required_elements]);
                            drop(buffer_guard);
                            if let Ok(guard) = ffi_enabled.try_lock() {
                                if *guard {
                                    //print!("ffi enabled\r");
                                    last_ffi_enabled = true;
                                    time_domain_to_frequency_domain(buffer.clone(), &mut planner);
                                } else {
                                    //print!("ffi disabled\r");
                                    last_ffi_enabled = false;
                                }
                            }
                            accumulation_buffer.drain(..required_elements);
                        } else if accumulation_buffer.len() > required_elements * 2{
                            let excess = accumulation_buffer.len() - required_elements;
                            accumulation_buffer.drain(..excess);
                        }
                    }
                }

                capture_client.ReleaseBuffer(num_frames)?;
            }
        }
    }
}

fn samples_to_f32(data: *const u8, count: usize, bits: u16, is_float: bool) -> Vec<f32> {
    unsafe {
        match (is_float, bits) {
            (true, 32) => {
                std::slice::from_raw_parts(data as *const f32, count).to_vec()
            }
            (false, 16) => {
                std::slice::from_raw_parts(data as *const i16, count)
                    .iter().map(|&s| s as f32 / 32768.0).collect()
            }
            (false, 32) => {
                std::slice::from_raw_parts(data as *const i32, count)
                    .iter().map(|&s| s as f32 / 2147483648.0).collect()
            }
            (false, 24) => {
                // Packed 24-bit, 3 bytes per sample
                (0..count).map(|i| {
                    let b = &std::slice::from_raw_parts(data, count * 3)[i*3..i*3+3];
                    let val = (b[0] as i32) | ((b[1] as i32) << 8) | ((b[2] as i32) << 16);
                    let val = if val & 0x800000 != 0 { val | !0xFFFFFF } else { val };
                    val as f32 / 8388608.0
                }).collect()
            }
            _ => vec![0.0; count] // unsupported format, silent fallback
        }
    }
}