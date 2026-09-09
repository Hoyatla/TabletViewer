//! COCO 80 class names, in the canonical order used by YOLOX / YOLOv5 / etc.
//!
//! Index 0 = `person`, 15 = `cat`, 16 = `dog`, 17 = `horse`, 18 = `sheep`,
//! 19 = `cow`, 20 = `elephant`, 21 = `bear`, 22 = `zebra`, 23 = `giraffe`,
//! 14 = `bird`. Indices are stable across all COCO-pretrained models, so the
//! same byte picks the same class in any of them.

pub const CLASSES: &[&str] = &[
    "person", "bicycle", "car", "motorcycle", "airplane", "bus", "train", "truck",
    "boat", "traffic light", "fire hydrant", "stop sign", "parking meter", "bench",
    "bird", "cat", "dog", "horse", "sheep", "cow", "elephant", "bear", "zebra",
    "giraffe", "backpack", "umbrella", "handbag", "tie", "suitcase", "frisbee",
    "skis", "snowboard", "sports ball", "kite", "baseball bat", "baseball glove",
    "skateboard", "surfboard", "tennis racket", "bottle", "wine glass", "cup",
    "fork", "knife", "spoon", "bowl", "banana", "apple", "sandwich", "orange",
    "broccoli", "carrot", "hot dog", "pizza", "donut", "cake", "chair", "couch",
    "potted plant", "bed", "dining table", "toilet", "tv", "laptop", "mouse",
    "remote", "keyboard", "cell phone", "microwave", "oven", "toaster", "sink",
    "refrigerator", "book", "clock", "vase", "scissors", "teddy bear",
    "hair drier", "toothbrush",
];

pub const PERSON: usize = 0;

/// Subset of [`CLASSES`] that contain common domestic or wild animals.
/// Useful for `?classes=animals` filtering in the HTTP API.
pub const ANIMAL_CLASS_IDS: &[usize] = &[
    14, // bird
    15, // cat
    16, // dog
    17, // horse
    18, // sheep
    19, // cow
    20, // elephant
    21, // bear
    22, // zebra
    23, // giraffe
];

pub fn is_animal(class_id: usize) -> bool {
    ANIMAL_CLASS_IDS.contains(&class_id)
}

pub fn name(class_id: usize) -> &'static str {
    CLASSES.get(class_id).copied().unwrap_or("unknown")
}
