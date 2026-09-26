#[cfg(test)]
mod tests {
    use super::*;

    struct Memory(Vec<u8>);

    impl GuestMemory for Memory {
        fn read(&self, address: u32, output: &mut [u8]) -> Result<(), &'static str> {
            let start = address as usize;
            output.copy_from_slice(
                self.0
                    .get(start..start + output.len())
                    .ok_or("texture test read out of range")?,
            );
            Ok(())
        }

        fn write(&mut self, address: u32, input: &[u8]) -> Result<(), &'static str> {
            let start = address as usize;
            self.0
                .get_mut(start..start + input.len())
                .ok_or("texture test write out of range")?
                .copy_from_slice(input);
            Ok(())
        }
    }

    struct WriteFailureMemory;

    impl GuestMemory for WriteFailureMemory {
        fn read(&self, _address: u32, _output: &mut [u8]) -> Result<(), &'static str> {
            Err("unexpected read")
        }

        fn write(&mut self, _address: u32, _input: &[u8]) -> Result<(), &'static str> {
            Err("injected output failure")
        }
    }

    struct SecondReadFailureMemory {
        bytes: Vec<u8>,
        reads: std::cell::Cell<u32>,
    }

    impl GuestMemory for SecondReadFailureMemory {
        fn read(&self, address: u32, output: &mut [u8]) -> Result<(), &'static str> {
            let reads = self.reads.get();
            self.reads.set(reads + 1);
            if reads != 0 {
                return Err("injected second-row read failure");
            }
            let start = address as usize;
            output.copy_from_slice(
                self.bytes
                    .get(start..start + output.len())
                    .ok_or("texture test read out of range")?,
            );
            Ok(())
        }

        fn write(&mut self, _address: u32, _input: &[u8]) -> Result<(), &'static str> {
            Err("unexpected write")
        }
    }

    fn names(memory: &Memory, address: usize, count: usize) -> Vec<u32> {
        memory.0[address..address + count * 4]
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .collect()
    }

    fn image(width: u32, height: u32, rgba: &[u8]) -> GlTextureImage {
        GlTextureImage {
            width,
            height,
            internal: 0x1908,
            rgba: rgba.to_vec(),
        }
    }

    #[test]
    fn generated_names_become_objects_only_when_bound_and_delete_unbinds() {
        let mut textures = GlTextures::default();
        let mut memory = Memory(vec![0; 32]);

        textures.generate(3, 4, &mut memory).unwrap();
        let generated = names(&memory, 4, 3);
        assert_eq!(generated, [1, 2, 3]);
        assert_eq!(textures.reserved.len(), 3);
        assert!(textures.objects.is_empty());

        textures.bind(generated[1]).unwrap();
        assert_eq!(textures.binding, 2);
        assert!(textures.objects.contains_key(&2));
        assert!(!textures.objects.contains_key(&1));

        // Binding an arbitrary nonzero name creates the object on first use.
        textures.bind(91).unwrap();
        assert!(textures.reserved.contains(&91));
        assert!(textures.objects.contains_key(&91));
        textures.delete(&[91, 0, generated[0]]);
        assert_eq!(textures.binding, 0);
        assert!(!textures.reserved.contains(&91));
        assert!(!textures.objects.contains_key(&91));
        assert!(!textures.reserved.contains(&generated[0]));
        assert!(textures.reserved.contains(&generated[2]));
    }

    #[test]
    fn failed_name_output_does_not_consume_or_reserve_names() {
        let mut textures = GlTextures::default();
        let mut failing_memory = WriteFailureMemory;

        assert_eq!(
            textures.generate(2, 4, &mut failing_memory),
            Err("injected output failure")
        );
        assert_eq!(textures.next, 1);
        assert!(textures.reserved.is_empty());

        let mut memory = Memory(vec![0; 8]);
        textures.generate(2, 0, &mut memory).unwrap();
        assert_eq!(names(&memory, 0, 2), [1, 2]);
        assert_eq!(textures.next, 3);
    }

    #[test]
    fn texture_names_and_bindings_are_isolated_per_context_storage() {
        let mut first = GlTextures::default();
        let mut second = GlTextures::default();
        first.bind(7).unwrap();
        first.set_image(0, image(1, 1, &[1, 2, 3, 4])).unwrap();

        second.bind(7).unwrap();
        assert!(second.object().levels.is_empty());
        second.set_image(0, image(1, 1, &[9, 8, 7, 6])).unwrap();

        assert_eq!(first.object().levels[&0].rgba, [1, 2, 3, 4]);
        assert_eq!(second.object().levels[&0].rgba, [9, 8, 7, 6]);
    }

    #[test]
    fn texture_levels_are_owned_and_replacing_one_does_not_touch_another() {
        let mut textures = GlTextures::default();
        textures.bind(7).unwrap();
        textures
            .set_image(
                0,
                image(
                    2,
                    2,
                    &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                ),
            )
            .unwrap();
        textures
            .set_image(1, image(1, 1, &[20, 21, 22, 23]))
            .unwrap();
        textures
            .set_image(
                0,
                image(
                    2,
                    2,
                    &[
                        30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45,
                    ],
                ),
            )
            .unwrap();

        let object = textures.object();
        assert_eq!(textures.bytes(), 20);
        assert_eq!(object.levels[&0].rgba[0..4], [30, 31, 32, 33]);
        assert_eq!(object.levels[&1].width, 1);
        assert_eq!(object.levels[&1].height, 1);
        assert_eq!(object.levels[&1].rgba, [20, 21, 22, 23]);
    }

    #[test]
    fn subimage_changes_only_its_rectangle_and_keeps_other_mips() {
        let mut textures = GlTextures::default();
        textures.bind(1).unwrap();
        textures.set_image(0, image(3, 2, &[0; 24])).unwrap();
        textures
            .set_image(1, image(1, 1, &[90, 91, 92, 93]))
            .unwrap();
        let memory = Memory(vec![0; 4].into_iter().chain([1, 2, 3, 4]).collect());

        textures
            .sub_image(0, [1, 1, 1, 1], 0x1908, GL_UNSIGNED_BYTE, 4, &memory)
            .unwrap();

        let object = textures.object();
        assert_eq!(
            object.levels[&0].rgba,
            [
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 0, 0, 0, 0,
            ]
        );
        assert_eq!(object.levels[&1].rgba, [90, 91, 92, 93]);
    }

    #[test]
    fn out_of_bounds_subimage_leaves_existing_image_unchanged() {
        let mut textures = GlTextures::default();
        textures.bind(1).unwrap();
        textures
            .set_image(
                0,
                image(
                    2,
                    2,
                    &[
                        10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
                    ],
                ),
            )
            .unwrap();
        let before = textures.object().levels[&0].rgba.clone();
        let memory = Memory(vec![0; 4].into_iter().chain([1, 2, 3, 4]).collect());

        assert_eq!(
            textures.sub_image(0, [1, 1, 2, 1], 0x1908, GL_UNSIGNED_BYTE, 4, &memory),
            Err("invalid subimage bounds/pointer")
        );
        assert_eq!(textures.object().levels[&0].rgba, before);
    }

    #[test]
    fn failed_later_subimage_row_leaves_existing_image_unchanged() {
        let mut textures = GlTextures::default();
        textures.bind(1).unwrap();
        textures
            .set_image(0, image(1, 2, &[10, 11, 12, 13, 14, 15, 16, 17]))
            .unwrap();
        let before = textures.object().levels[&0].rgba.clone();
        let memory = SecondReadFailureMemory {
            bytes: vec![0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8],
            reads: std::cell::Cell::new(0),
        };

        assert_eq!(
            textures.sub_image(0, [0, 0, 1, 2], 0x1908, GL_UNSIGNED_BYTE, 4, &memory),
            Err("injected second-row read failure")
        );
        assert_eq!(textures.object().levels[&0].rgba, before);
    }

    #[test]
    fn sampled_draw_gate_rejects_default_or_unrepresentable_texture_state() {
        let mut textures = GlTextures::default();
        textures.bind(1).unwrap();
        textures.set_image(0, image(1, 1, &[1, 2, 3, 255])).unwrap();

        // GL defaults use mipmapped/linear filtering, which the authenticated
        // sampled route cannot approximate.
        assert!(gl_texture_draw_image(&textures).is_err());
        {
            let object = textures.object_mut();
            object.min_filter = 0x2600;
            object.mag_filter = 0x2600;
        }
        assert!(gl_texture_draw_image(&textures).is_ok());

        textures.object_mut().mag_filter = 0x2601;
        assert!(gl_texture_draw_image(&textures).is_err());
        textures.object_mut().mag_filter = 0x2600;
        textures.object_mut().wrap_s = 0x812f;
        assert!(gl_texture_draw_image(&textures).is_err());
        textures.object_mut().wrap_s = 0x2901;
        textures.object_mut().levels.get_mut(&0).unwrap().rgba[3] = 7;
        assert!(gl_texture_draw_image(&textures).is_err());
        textures.object_mut().levels.get_mut(&0).unwrap().rgba[3] = 255;
        textures.env_mode = 0x2101;
        assert!(gl_texture_draw_image(&textures).is_err());
        textures.env_mode = 0x1e01;
        assert!(gl_texture_draw_image(&textures).is_ok());
    }

    #[test]
    fn unpack_row_length_skip_and_alignment_select_the_expected_rgb_rows() {
        // Three RGB pixels per row followed by three padding bytes: alignment 4
        // rounds 9 source bytes up to a 12-byte stride.
        let mut source = Vec::new();
        for base in [0u8, 10, 20] {
            source.extend((base..base + 9).into_iter());
            source.extend([0xee; 3]);
        }
        let memory = Memory(vec![0; 4].into_iter().chain(source).collect());
        let unpack = GlUnpack {
            alignment: 4,
            row_length: 3,
            skip_rows: 1,
            skip_pixels: 1,
            ..Default::default()
        };

        let rgba =
            gl_texture_pixels(&memory, 4, 2, 2, 0x1907, GL_UNSIGNED_BYTE, unpack, 0x1908).unwrap();
        assert_eq!(
            rgba,
            vec![
                13, 14, 15, 255, 16, 17, 18, 255, 23, 24, 25, 255, 26, 27, 28, 255,
            ]
        );
    }

    #[test]
    fn bgra_and_luminance_alpha_are_canonicalized_to_owned_rgba8() {
        let memory = Memory(vec![0, 0, 0, 0, 0x33, 0x22, 0x11, 0x44, 7, 9]);
        let bgra = gl_texture_pixels(
            &memory,
            4,
            1,
            1,
            0x80e1,
            GL_UNSIGNED_BYTE,
            GlUnpack::default(),
            0x1908,
        )
        .unwrap();
        let luminance_alpha = gl_texture_pixels(
            &memory,
            8,
            1,
            1,
            0x190a,
            GL_UNSIGNED_BYTE,
            GlUnpack::default(),
            0x190a,
        )
        .unwrap();
        assert_eq!(bgra, [0x11, 0x22, 0x33, 0x44]);
        assert_eq!(luminance_alpha, [7, 7, 7, 9]);
    }

    #[test]
    fn pathological_unpack_layout_is_rejected_without_integer_wrap_or_panic() {
        let memory = Memory(vec![0; 4]);
        let unpack = GlUnpack {
            alignment: 4,
            row_length: i32::MAX as u32,
            skip_rows: i32::MAX as u32,
            ..Default::default()
        };
        let result = std::panic::catch_unwind(|| {
            gl_texture_pixels(
                &memory,
                1,
                1,
                GL_MAX_TEXTURE_EDGE,
                0x1908,
                GL_UNSIGNED_BYTE,
                unpack,
                0x1908,
            )
        });
        assert!(matches!(result, Ok(Err(_))));
    }
}
