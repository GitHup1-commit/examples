use anyhow::anyhow;
use candid::CandidType;
use prost::Message;
use serde::Deserialize;
use std::cell::RefCell;
use tract_ndarray::s;
use tract_onnx::prelude::*;

type Model = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

thread_local! {
    static ULTRAFACE: RefCell<Option<Model>> = RefCell::new(None);
    static FACEREC: RefCell<Option<Model>> = RefCell::new(None);
}

#[derive(CandidType, Deserialize, Clone)]
pub struct BoundingBox {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl BoundingBox {
    fn new(raw: &[f32]) -> Self {
        Self {
            left: raw[0],
            top: raw[1],
            right: raw[2],
            bottom: raw[3],
        }
    }
}

#[derive(CandidType, Deserialize, Clone)]
pub struct Embedding {
    v0: Vec<f32>,
}

const ULTRAFACE_ONNX: &'static [u8] = include_bytes!("../assets/version-RFB-320.onnx");
const FACEREC_ONNX: &'static [u8] = include_bytes!("../assets/facerec.onnx");

fn setup_ultraface() -> TractResult<()> {
    let bytes = bytes::Bytes::from_static(ULTRAFACE_ONNX);
    let proto: tract_onnx::pb::ModelProto = tract_onnx::pb::ModelProto::decode(bytes)?;
    let ultraface = tract_onnx::onnx()
        .model_for_proto_model(&proto)?
        .into_optimized()?
        .into_runnable()?;
    ULTRAFACE.with_borrow_mut(|m| {
        *m = Some(ultraface);
    });
    Ok(())
}

fn setup_facerec() -> TractResult<()> {
    let bytes = bytes::Bytes::from_static(FACEREC_ONNX);
    let proto: tract_onnx::pb::ModelProto = tract_onnx::pb::ModelProto::decode(bytes)?;
    let facerec = tract_onnx::onnx()
        .model_for_proto_model(&proto)?
        .into_optimized()?
        .into_runnable()?;
    FACEREC.with_borrow_mut(|m| {
        *m = Some(facerec);
    });
    Ok(())
}

pub fn setup() -> TractResult<()> {
    setup_ultraface()?;
    setup_facerec()
}

/// Runs the model on the given image and returns top three labels.
pub fn detect(image: Vec<u8>) -> Result<(BoundingBox, f32), anyhow::Error> {
    ULTRAFACE.with_borrow(|model| {
        ic_cdk::api::print("started!");
        let model = model.as_ref().unwrap();
        let image = image::load_from_memory(&image)?.to_rgb8();

        // The model accepts an image of size 320x240px.
        let image =
            image::imageops::resize(&image, 320, 240, ::image::imageops::FilterType::Triangle);

        // Preprocess the input according to
        // https://github.com/onnx/models/tree/main/validated/vision/classification/mobilenet#preprocessing.
        const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
        const STD: [f32; 3] = [0.229, 0.224, 0.225];
        let tensor = tract_ndarray::Array4::from_shape_fn((1, 3, 240, 320), |(_, c, y, x)| {
            (image[(x as u32, y as u32)][c] as f32 / 255.0 - MEAN[c]) / STD[c]
        });

        ic_cdk::api::print("before run!");
        let result = model.run(tvec!(Tensor::from(tensor).into()))?;
        ic_cdk::api::print("after run!");

        let confidences = result[0]
            .to_array_view::<f32>()?
            .slice(s![0, .., 1])
            .to_vec();

        // Extract relative coordinates of bounding boxes
        let boxes: Vec<_> = result[1].to_array_view::<f32>()?.iter().cloned().collect();

        let boxes: Vec<_> = boxes.chunks(4).map(BoundingBox::new).collect();

        let boxes: Vec<_> = boxes.iter().zip(confidences.iter()).collect();

        ic_cdk::api::print("almsot there!");

        let best = boxes
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .ok_or(anyhow!("No face detected"))?;

        let best = (best.0.clone(), best.1.clone());
        Ok(best)
    })
}

/// Runs the model on the given image and returns top three labels.
pub fn embedding(image: Vec<u8>) -> Result<Embedding, anyhow::Error> {
    FACEREC.with_borrow(|model| {
        let model = model.as_ref().unwrap();
        let image = image::load_from_memory(&image)?.to_rgb8();

        // The model accepts an image of size 140x140px.
        let image =
            image::imageops::resize(&image, 140, 140, ::image::imageops::FilterType::Triangle);

        let tensor = tract_ndarray::Array4::from_shape_fn((1, 3, 140, 140), |(_, c, y, x)| {
            image[(x as u32, y as u32)][c] as f32 / 255.0
        });

        let result = model.run(tvec!(Tensor::from(tensor).into()))?;

        let v0 = result[0]
            .to_array_view::<f32>()?
            .into_iter()
            .cloned()
            .collect();

        Ok(Embedding { v0 })
    })
}
