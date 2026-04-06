import { useEffect, useRef, useState } from "react";
import {
  Alert,
  Dimensions,
  FlatList,
  Image,
  Modal,
  ScrollView,
  Text,
  TouchableOpacity,
  View,
} from "react-native";
import * as ImagePicker from "expo-image-picker";
import { ChevronLeft, ChevronRight, Plus, Trash2, X, Image as ImageIcon } from "lucide-react-native";
import { useGalleryStore } from "../../../stores/galleryStore";
import { GalleryImage } from "../../../lib/types";
import { buildSseUrl } from "../../../api/client";
import { useConnectionStore } from "../../../stores/connectionStore";

const SCREEN_WIDTH = Dimensions.get("window").width;
const TILE_SIZE = (SCREEN_WIDTH - 4) / 3;

export default function GalleryTab() {
  const { allImages, fetchAllImages, deleteImage, refresh } = useGalleryStore();
  const { host, token } = useConnectionStore();
  const [lightboxIndex, setLightboxIndex] = useState<number | null>(null);
  const scrollRef = useRef<ScrollView>(null);

  useEffect(() => {
    fetchAllImages();
  }, []);

  const handleUpload = async () => {
    const { status } = await ImagePicker.requestMediaLibraryPermissionsAsync();
    if (status !== "granted") return;
    const result = await ImagePicker.launchImageLibraryAsync({
      mediaTypes: ImagePicker.MediaTypeOptions.Images,
      allowsMultipleSelection: true,
      quality: 0.9,
    });
    if (result.canceled) return;

    for (const asset of result.assets) {
      const form = new FormData();
      form.append("file", { uri: asset.uri, name: "upload.jpg", type: "image/jpeg" } as unknown as Blob);
      await fetch(`${host}/api/gallery/upload`, {
        method: "POST",
        headers: token ? { Authorization: `Bearer ${token}` } : {},
        body: form,
      });
    }
    await refresh();
  };

  const handleDelete = (img: GalleryImage) => {
    Alert.alert("Delete image?", "This cannot be undone.", [
      { text: "Cancel", style: "cancel" },
      {
        text: "Delete",
        style: "destructive",
        onPress: async () => {
          await deleteImage(img.id);
          if (lightboxIndex !== null) setLightboxIndex(null);
        },
      },
    ]);
  };

  const getImageUrl = (img: GalleryImage) => {
    if (img.data_url) return img.data_url;
    return `${host}/api/gallery/${img.id}/image`;
  };

  return (
    <View className="flex-1 bg-background">
      {/* Header */}
      <View className="flex-row items-center justify-between px-4 pt-14 pb-3 border-b border-border">
        <Text className="text-foreground text-xl font-semibold">Gallery</Text>
        <TouchableOpacity
          onPress={handleUpload}
          className="w-9 h-9 bg-primary rounded-xl items-center justify-center"
          activeOpacity={0.8}
        >
          <Plus size={18} color="#1e1e2e" />
        </TouchableOpacity>
      </View>

      {allImages.length === 0 ? (
        <View className="flex-1 items-center justify-center gap-4">
          <ImageIcon size={48} color="#313244" />
          <Text className="text-muted text-base">No images yet</Text>
          <TouchableOpacity onPress={handleUpload} className="bg-primary rounded-xl px-6 py-3">
            <Text className="text-background font-medium">Upload images</Text>
          </TouchableOpacity>
        </View>
      ) : (
        <FlatList
          data={allImages}
          keyExtractor={(item) => item.id}
          numColumns={3}
          renderItem={({ item, index }) => (
            <TouchableOpacity
              onPress={() => setLightboxIndex(index)}
              style={{ width: TILE_SIZE, height: TILE_SIZE, margin: 0.5 }}
              activeOpacity={0.8}
            >
              <Image
                source={{ uri: getImageUrl(item) }}
                style={{ width: "100%", height: "100%" }}
                resizeMode="cover"
              />
            </TouchableOpacity>
          )}
        />
      )}

      {/* Lightbox */}
      <Modal
        visible={lightboxIndex !== null}
        transparent
        animationType="fade"
        onRequestClose={() => setLightboxIndex(null)}
      >
        <View className="flex-1 bg-black/95 items-center justify-center">
          {lightboxIndex !== null && allImages[lightboxIndex] && (
            <>
              <Image
                source={{ uri: getImageUrl(allImages[lightboxIndex]) }}
                style={{ width: SCREEN_WIDTH, height: SCREEN_WIDTH }}
                resizeMode="contain"
              />
              {/* Controls */}
              <View className="absolute top-14 right-4 gap-3">
                <TouchableOpacity
                  onPress={() => setLightboxIndex(null)}
                  className="w-10 h-10 bg-black/60 rounded-full items-center justify-center"
                >
                  <X size={20} color="#fff" />
                </TouchableOpacity>
                <TouchableOpacity
                  onPress={() => handleDelete(allImages[lightboxIndex])}
                  className="w-10 h-10 bg-black/60 rounded-full items-center justify-center"
                >
                  <Trash2 size={18} color="#f38ba8" />
                </TouchableOpacity>
              </View>
              {/* Prev/Next arrows */}
              <View className="absolute inset-x-0 top-1/2 flex-row justify-between px-4">
                {lightboxIndex > 0 && (
                  <TouchableOpacity
                    onPress={() => setLightboxIndex(lightboxIndex - 1)}
                    className="w-10 h-10 bg-black/60 rounded-full items-center justify-center"
                  >
                    <ChevronLeft size={22} color="#fff" />
                  </TouchableOpacity>
                )}
                <View className="flex-1" />
                {lightboxIndex < allImages.length - 1 && (
                  <TouchableOpacity
                    onPress={() => setLightboxIndex(lightboxIndex + 1)}
                    className="w-10 h-10 bg-black/60 rounded-full items-center justify-center"
                  >
                    <ChevronRight size={22} color="#fff" />
                  </TouchableOpacity>
                )}
              </View>
              {/* Counter */}
              <View className="absolute bottom-12 self-center">
                <Text className="text-white/70 text-sm">
                  {lightboxIndex + 1} / {allImages.length}
                </Text>
              </View>
            </>
          )}
        </View>
      </Modal>
    </View>
  );
}
